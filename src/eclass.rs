use crate::no_std_prelude::*;
use core::fmt::Debug;
use core::iter::ExactSizeIterator;

use crate::*;

/// An equivalence class of enodes.
#[non_exhaustive]
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-1", derive(serde::Serialize, serde::Deserialize))]
pub struct EClass<L, D> {
    /// This eclass's id.
    pub id: Id,
    /// The equivalent enodes in this equivalence class.
    pub nodes: Vec<L>,
    /// The analysis data associated with this eclass.
    ///
    /// Modifying this field will _not_ cause changes to propagate through the e-graph.
    /// Prefer [`EGraph::set_analysis_data`] instead.
    pub data: D,
    /// The original Ids of parent enodes.
    pub(crate) parents: Vec<Id>,
    /// Index over the runs of same-discriminant nodes in the sorted `nodes`
    /// vector: `(hash of the discriminant, start offset of the run)`, in node
    /// order. Rebuilt by `EGraph::rebuild` and cleared whenever the class is
    /// mutated outside of a rebuild. Empty means "no index" and all queries
    /// fall back to scanning/searching `nodes` directly.
    #[cfg_attr(feature = "serde-1", serde(skip))]
    pub(crate) discrim_groups: Vec<(u64, u32)>,
}

impl<L, D> EClass<L, D> {
    /// Returns `true` if the `eclass` is empty.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Returns the number of enodes in this eclass.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Iterates over the enodes in this eclass.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &L> {
        self.nodes.iter()
    }

    /// Iterates over the non-canonical ids of parent enodes of this eclass.
    pub fn parents(&self) -> impl ExactSizeIterator<Item = Id> + '_ {
        self.parents.iter().copied()
    }
}

/// Slot value in [`ClassMap::index`] marking an id with no e-class.
const NO_CLASS: u32 = u32::MAX;

/// Maps canonical [`Id`]s to [`EClass`]es.
///
/// Ids are dense small integers handed out by the union-find, so instead of a
/// hash map this is a slot map: `index` maps an id to a slot in the dense
/// `list`. Lookups are two array loads (no hashing), and iteration over the
/// classes is a contiguous scan.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-1", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct ClassMap<L, D> {
    index: Vec<u32>,
    list: Vec<EClass<L, D>>,
}

impl<L, D> Default for ClassMap<L, D> {
    fn default() -> Self {
        ClassMap {
            index: Vec::new(),
            list: Vec::new(),
        }
    }
}

impl<L, D> ClassMap<L, D> {
    pub(crate) fn len(&self) -> usize {
        self.list.len()
    }

    #[inline]
    pub(crate) fn get(&self, id: Id) -> Option<&EClass<L, D>> {
        let slot = *self.index.get(usize::from(id))?;
        if slot == NO_CLASS {
            return None;
        }
        // SAFETY: every non-NO_CLASS slot in `index` is a valid position in
        // `list`: `insert` writes `list.len()` right before pushing, and
        // `remove` rewrites the slot of the element it swaps into place.
        unsafe { branches::assume((slot as usize) < self.list.len()) };
        Some(&self.list[slot as usize])
    }

    #[inline]
    pub(crate) fn get_mut(&mut self, id: Id) -> Option<&mut EClass<L, D>> {
        let slot = *self.index.get(usize::from(id))?;
        if slot == NO_CLASS {
            return None;
        }
        // SAFETY: see `get`
        unsafe { branches::assume((slot as usize) < self.list.len()) };
        Some(&mut self.list[slot as usize])
    }

    pub(crate) fn insert(&mut self, id: Id, class: EClass<L, D>) {
        let i = usize::from(id);
        if i >= self.index.len() {
            self.index.resize(i + 1, NO_CLASS);
        }
        debug_assert_eq!(self.index[i], NO_CLASS, "double insert for {id}");
        assert!(self.list.len() < NO_CLASS as usize);
        self.index[i] = self.list.len() as u32;
        self.list.push(class);
    }

    pub(crate) fn remove(&mut self, id: Id) -> Option<EClass<L, D>> {
        let slot = core::mem::replace(self.index.get_mut(usize::from(id))?, NO_CLASS);
        if slot == NO_CLASS {
            return None;
        }
        let class = self.list.swap_remove(slot as usize);
        if let Some(moved) = self.list.get(slot as usize) {
            self.index[usize::from(moved.id)] = slot;
        }
        Some(class)
    }

    pub(crate) fn keys(&self) -> impl ExactSizeIterator<Item = Id> + '_ {
        self.list.iter().map(|class| class.id)
    }

    pub(crate) fn iter(&self) -> impl ExactSizeIterator<Item = (Id, &EClass<L, D>)> {
        self.list.iter().map(|class| (class.id, class))
    }

    pub(crate) fn values(&self) -> impl ExactSizeIterator<Item = &EClass<L, D>> {
        self.list.iter()
    }

    pub(crate) fn values_mut(&mut self) -> impl ExactSizeIterator<Item = &mut EClass<L, D>> {
        self.list.iter_mut()
    }

    /// Maps every class into a new `ClassMap`, preserving ids.
    ///
    /// The mapping function must keep [`EClass::id`] unchanged.
    pub(crate) fn map<L2, D2>(
        self,
        f: impl FnMut(EClass<L, D>) -> EClass<L2, D2>,
    ) -> ClassMap<L2, D2> {
        ClassMap {
            index: self.index,
            list: self.list.into_iter().map(f).collect(),
        }
    }
}

impl<L, D> core::ops::Index<Id> for ClassMap<L, D> {
    type Output = EClass<L, D>;
    #[inline]
    fn index(&self, id: Id) -> &Self::Output {
        self.get(id)
            .unwrap_or_else(|| panic!("Invalid id {}", id))
    }
}

impl<L: Language, D> EClass<L, D> {
    /// Iterates over the childless enodes in this eclass.
    pub fn leaves(&self) -> impl Iterator<Item = &L> {
        self.nodes.iter().filter(|&n| n.is_leaf())
    }

    /// Asserts that the childless enodes in this eclass are unique.
    pub fn assert_unique_leaves(&self)
    where
        L: Language,
    {
        let mut leaves = self.leaves();
        if let Some(first) = leaves.next() {
            assert!(
                leaves.all(|l| l == first),
                "Different leaves in eclass {}: {:?}",
                self.id,
                self.leaves().collect::<crate::util::HashSet<_>>()
            );
        }
    }

    /// Run some function on each matching e-node in this class.
    pub fn for_each_matching_node<Err>(
        &self,
        node: &L,
        mut f: impl FnMut(&L) -> Result<(), Err>,
    ) -> Result<(), Err>
    where
        L: Language,
    {
        if !self.discrim_groups.is_empty() {
            // Fresh from a rebuild: `nodes` is sorted and `discrim_groups` indexes
            // its same-discriminant runs. Find the run by hash, re-check its head
            // node against collisions, and scan only that run.
            use core::hash::BuildHasher;
            let discrim = node.discriminant();
            let query_hash = crate::util::BuildHasher::default().hash_one(&discrim);
            let groups = &self.discrim_groups;
            for (i, &(hash, start)) in groups.iter().enumerate() {
                if hash == query_hash {
                    let start = start as usize;
                    // SAFETY: a non-empty group index is in sync with `nodes`:
                    // `rebuild_classes` builds it from in-bounds, strictly
                    // increasing positions, and every path that can mutate
                    // `nodes` outside a rebuild clears it (`perform_union`
                    // internally; `classes_mut` / `IndexMut` for user code).
                    unsafe { branches::assume(start < self.nodes.len()) };
                    if self.nodes[start].discriminant() != discrim {
                        continue; // hash collision between discriminants
                    }
                    let end = groups
                        .get(i + 1)
                        .map_or(self.nodes.len(), |&(_, s)| s as usize);
                    // SAFETY: starts are strictly increasing and in bounds
                    // (see above), so `start <= end <= nodes.len()`.
                    unsafe { branches::assume(start <= end && end <= self.nodes.len()) };
                    let run = &self.nodes[start..end];
                    return if node.is_leaf() {
                        // for a leaf, `matches` is equality: at most one match
                        match run.binary_search(node) {
                            Ok(i) => f(&run[i]),
                            Err(_) => Ok(()),
                        }
                    } else {
                        run.iter().filter(|n| node.matches(n)).try_for_each(f)
                    };
                }
            }
            Ok(())
        } else if self.nodes.len() < 50 {
            self.nodes
                .iter()
                .filter(|n| node.matches(n))
                .try_for_each(f)
        } else if node.is_leaf() {
            debug_assert!(self.nodes.windows(2).all(|w| w[0] < w[1]));
            // for a leaf, `matches` is equality: binary search finds the only match
            match self.nodes.binary_search(node) {
                Ok(i) => f(&self.nodes[i]),
                Err(_) => Ok(()),
            }
        } else {
            debug_assert!(node.all(|id| id == Id::from(0)));
            debug_assert!(self.nodes.windows(2).all(|w| w[0] < w[1]));
            let mut start = self.nodes.binary_search(node).unwrap_or_else(|i| i);
            let discrim = node.discriminant();
            while start > 0 {
                if self.nodes[start - 1].discriminant() == discrim {
                    start -= 1;
                } else {
                    break;
                }
            }
            let mut matching = self.nodes[start..]
                .iter()
                .take_while(|&n| n.discriminant() == discrim)
                .filter(|n| node.matches(n));
            debug_assert_eq!(
                matching.clone().count(),
                self.nodes.iter().filter(|n| node.matches(n)).count(),
                "matching node {:?}\nstart={}\n{:?} != {:?}\nnodes: {:?}",
                node,
                start,
                matching.clone().collect::<HashSet<_>>(),
                self.nodes
                    .iter()
                    .filter(|n| node.matches(n))
                    .collect::<HashSet<_>>(),
                self.nodes
            );
            matching.try_for_each(&mut f)
        }
    }
}
