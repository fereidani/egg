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
    /// vector: `(group_hash of the discriminant, start offset of the run)`,
    /// in node order. Rebuilt by `EGraph::rebuild` and cleared whenever the
    /// class is mutated outside of a rebuild. Empty means "no index" and all
    /// queries fall back to scanning `nodes`.
    #[cfg_attr(feature = "serde-1", serde(skip))]
    pub(crate) discrim_groups: DiscrimGroups,
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

/// Group index of an [`EClass`]; up to two runs are stored inline, in the
/// space of a `Vec`.
pub(crate) type DiscrimGroups = smallvec::SmallVec<[(u32, u32); 2]>;

/// The part of a [`discriminant_hash`] kept in a group index.
#[inline]
fn group_hash(discriminant_hash: u64) -> u32 {
    (discriminant_hash >> 32) as u32
}

/// Hash of a discriminant, as used by group indexes and signatures.
#[inline]
pub(crate) fn discriminant_hash<T: core::hash::Hash>(discriminant: &T) -> u64 {
    let mut hasher = DiscriminantHasher(0);
    discriminant.hash(&mut hasher);
    core::hash::Hasher::finish(&hasher)
}

/// Folds the written integers and multiplies by the golden ratio (Fibonacci
/// hashing). A discriminant is usually one small integer; distinct ones get
/// distinct hashes, and up to 20 consecutive ones distinct [`signature_bit`]s.
struct DiscriminantHasher(u64);

impl core::hash::Hasher for DiscriminantHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_u64(u64::from(byte));
        }
    }

    fn write_u8(&mut self, i: u8) {
        self.write_u64(u64::from(i));
    }

    fn write_u16(&mut self, i: u16) {
        self.write_u64(u64::from(i));
    }

    fn write_u32(&mut self, i: u32) {
        self.write_u64(u64::from(i));
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.0 = self.0.rotate_left(5) ^ i;
    }

    fn write_usize(&mut self, i: usize) {
        self.write_u64(i as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.0.wrapping_mul(0x9E37_79B9_7F4A_7C15)
    }
}

/// Position value in a [`Slot`] marking an id with no e-class.
const NO_CLASS: u32 = u32::MAX;

/// Signature that rules nothing out; see [`ClassMap::signature`].
pub(crate) const ANY_DISCRIMINANT: u32 = u32::MAX;

/// Maps a [`discriminant_hash`] to its bit in a class signature.
#[inline]
pub(crate) fn signature_bit(discriminant_hash: u64) -> u32 {
    1 << (discriminant_hash >> 59)
}

/// One entry of [`ClassMap::index`].
#[derive(Debug, Clone, Copy)]
struct Slot {
    /// Position of the class in [`ClassMap::list`], or [`NO_CLASS`].
    pos: u32,
    /// See [`ClassMap::signature`].
    sig: u32,
}

impl Slot {
    const EMPTY: Slot = Slot {
        pos: NO_CLASS,
        sig: ANY_DISCRIMINANT,
    };
}

/// Maps canonical [`Id`]s to [`EClass`]es.
///
/// Ids are dense small integers handed out by the union-find, so instead of a
/// hash map this is a slot map: `index` maps an id to a slot in the dense
/// `list`. Lookups are two array loads (no hashing), and iteration over the
/// classes is a contiguous scan.
///
/// Each slot also holds a signature of its class's discriminants, so
/// e-matching can reject a class without loading it.
#[derive(Debug, Clone)]
pub(crate) struct ClassMap<L, D> {
    index: Vec<Slot>,
    list: Vec<EClass<L, D>>,
}

/// Hand-written to keep the derived format: `index` holds only positions, as
/// `EGraph::rebuild` recomputes the signatures.
#[cfg(feature = "serde-1")]
impl<L, D> serde::Serialize for ClassMap<L, D>
where
    L: serde::Serialize,
    D: serde::Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct as _;

        struct Positions<'a>(&'a [Slot]);

        impl serde::Serialize for Positions<'_> {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                serializer.collect_seq(self.0.iter().map(|slot| slot.pos))
            }
        }

        let mut state = serializer.serialize_struct("ClassMap", 2)?;
        state.serialize_field("index", &Positions(&self.index))?;
        state.serialize_field("list", &self.list)?;
        state.end()
    }
}

/// Hand-written to validate untrusted input: `get` and `get_mut` assume every
/// non-[`NO_CLASS`] slot is in bounds.
#[cfg(feature = "serde-1")]
impl<'de, L, D> serde::Deserialize<'de> for ClassMap<L, D>
where
    L: serde::Deserialize<'de>,
    D: serde::Deserialize<'de>,
{
    fn deserialize<De>(deserializer: De) -> Result<Self, De::Error>
    where
        De: serde::Deserializer<'de>,
    {
        use serde::de::Error as _;

        // Same shape as `serialize` writes.
        #[derive(serde::Deserialize)]
        struct Fields<L, D> {
            index: Vec<u32>,
            list: Vec<EClass<L, D>>,
        }

        let Fields { index, list } = Fields::deserialize(deserializer)?;
        if list.len() >= NO_CLASS as usize {
            return Err(De::Error::custom("too many e-classes for the class map"));
        }
        // every occupied slot names the class at its position, which also makes
        // the slots injective
        for (id, &pos) in index.iter().enumerate() {
            if pos == NO_CLASS {
                continue;
            }
            let class = list
                .get(pos as usize)
                .ok_or_else(|| De::Error::custom("class map slot is out of range"))?;
            if usize::from(class.id) != id {
                return Err(De::Error::custom("class map slot names the wrong e-class"));
            }
        }
        // and every class is reachable by its own id
        for (position, class) in list.iter().enumerate() {
            if index.get(usize::from(class.id)) != Some(&(position as u32)) {
                return Err(De::Error::custom("class map is missing a slot"));
            }
        }
        // Signatures are not serialized.
        let index = index
            .into_iter()
            .map(|pos| Slot {
                pos,
                sig: ANY_DISCRIMINANT,
            })
            .collect();
        Ok(ClassMap { index, list })
    }
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
        let pos = self.index.get(usize::from(id))?.pos;
        if pos == NO_CLASS {
            return None;
        }
        // SAFETY: every non-NO_CLASS position in `index` is a valid position
        // in `list`: `insert` writes `list.len()` right before pushing, and
        // `remove` rewrites the position of the element it swaps into place.
        unsafe { branches::assume((pos as usize) < self.list.len()) };
        Some(&self.list[pos as usize])
    }

    #[inline]
    pub(crate) fn get_mut(&mut self, id: Id) -> Option<&mut EClass<L, D>> {
        let pos = self.index.get(usize::from(id))?.pos;
        if pos == NO_CLASS {
            return None;
        }
        // SAFETY: see `get`
        unsafe { branches::assume((pos as usize) < self.list.len()) };
        Some(&mut self.list[pos as usize])
    }

    /// Bloom filter over the discriminants in the class of `id`: each one's
    /// [`signature_bit`] is set. Exact while the class's group index is valid;
    /// [`ANY_DISCRIMINANT`] otherwise, and for ids without a class.
    #[inline]
    pub(crate) fn signature(&self, id: Id) -> u32 {
        self.index
            .get(usize::from(id))
            .map_or(ANY_DISCRIMINANT, |slot| slot.sig)
    }

    /// Sets the signature of the class of `id`, if it has one.
    pub(crate) fn set_signature(&mut self, id: Id, sig: u32) {
        if let Some(slot) = self.index.get_mut(usize::from(id))
            && slot.pos != NO_CLASS
        {
            slot.sig = sig;
        }
    }

    /// Resets every signature to [`ANY_DISCRIMINANT`].
    pub(crate) fn clear_signatures(&mut self) {
        for slot in &mut self.index {
            slot.sig = ANY_DISCRIMINANT;
        }
    }

    pub(crate) fn insert(&mut self, id: Id, class: EClass<L, D>) {
        let i = usize::from(id);
        if i >= self.index.len() {
            self.index.resize(i + 1, Slot::EMPTY);
        }
        debug_assert_eq!(self.index[i].pos, NO_CLASS, "double insert for {id}");
        assert!(self.list.len() < NO_CLASS as usize);
        self.index[i] = Slot {
            pos: self.list.len() as u32,
            sig: ANY_DISCRIMINANT,
        };
        self.list.push(class);
    }

    pub(crate) fn remove(&mut self, id: Id) -> Option<EClass<L, D>> {
        let slot = core::mem::replace(self.index.get_mut(usize::from(id))?, Slot::EMPTY);
        if slot.pos == NO_CLASS {
            return None;
        }
        let class = self.list.swap_remove(slot.pos as usize);
        if let Some(moved) = self.list.get(slot.pos as usize) {
            self.index[usize::from(moved.id)].pos = slot.pos;
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

    /// Calls `f` on every class and makes its result the class's signature.
    pub(crate) fn update_each(&mut self, mut f: impl FnMut(&mut EClass<L, D>) -> u32) {
        for class in &mut self.list {
            let sig = f(class);
            self.index[usize::from(class.id)].sig = sig;
        }
    }

    /// Maps every class into a new `ClassMap`, preserving ids.
    ///
    /// The mapping function must keep [`EClass::id`] unchanged. Signatures are
    /// reset, as the new language has other discriminants.
    pub(crate) fn map<L2, D2>(
        self,
        f: impl FnMut(EClass<L, D>) -> EClass<L2, D2>,
    ) -> ClassMap<L2, D2> {
        let mut map = ClassMap {
            index: self.index,
            list: self.list.into_iter().map(f).collect(),
        };
        map.clear_signatures();
        map
    }
}

impl<L, D> core::ops::Index<Id> for ClassMap<L, D> {
    type Output = EClass<L, D>;
    #[inline]
    fn index(&self, id: Id) -> &Self::Output {
        self.get(id).unwrap_or_else(|| panic!("Invalid id {}", id))
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
        f: impl FnMut(&L) -> Result<(), Err>,
    ) -> Result<(), Err>
    where
        L: Language,
    {
        let query_hash = discriminant_hash(&node.discriminant());
        self.for_each_matching_node_hashed(node, query_hash, f)
    }

    /// [`EClass::for_each_matching_node`], given the [`discriminant_hash`] of
    /// `node`'s discriminant.
    pub(crate) fn for_each_matching_node_hashed<Err>(
        &self,
        node: &L,
        query_hash: u64,
        mut f: impl FnMut(&L) -> Result<(), Err>,
    ) -> Result<(), Err>
    where
        L: Language,
    {
        if !self.discrim_groups.is_empty() {
            // Fresh from a rebuild: `nodes` is sorted and `discrim_groups` indexes
            // its same-discriminant runs, one per discriminant. Find the run by
            // hash, re-check its head node against collisions, and scan only that run.
            let discrim = node.discriminant();
            let query_hash = group_hash(query_hash);
            let groups = &self.discrim_groups;
            for (i, &(hash, start)) in groups.iter().enumerate() {
                if hash == query_hash {
                    let start = start as usize;
                    // SAFETY: a non-empty group index is in sync with `nodes`:
                    // `index_discriminants` builds it from in-bounds, strictly
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
                        run.binary_search(node).map_or(Ok(()), |i| f(&run[i]))
                    } else {
                        run.iter().filter(|n| node.matches(n)).try_for_each(f)
                    };
                }
            }
            Ok(())
        } else {
            // no index, so nothing is known about the order of `nodes`
            self.nodes
                .iter()
                .filter(|n| node.matches(n))
                .try_for_each(f)
        }
    }

    /// Rebuilds the group index over the sorted, deduplicated `nodes` and
    /// returns the class's signature, calling `each_discriminant` once per run
    /// of equal discriminants.
    ///
    /// `Language` does not require `Ord` to keep equal discriminants together;
    /// a class where it splits one into several runs is left unindexed.
    pub(crate) fn index_discriminants(
        &mut self,
        mut each_discriminant: impl FnMut(&L::Discriminant),
    ) -> u32
    where
        L: Language,
    {
        let mut groups = core::mem::take(&mut self.discrim_groups);
        groups.clear();
        let mut sig = 0;
        let mut split = false;
        let mut prev: Option<L::Discriminant> = None;
        for (i, n) in self.nodes.iter().enumerate() {
            let discrim = n.discriminant();
            if prev.as_ref() == Some(&discrim) {
                continue;
            }
            let hash = discriminant_hash(&discrim);
            let bit = signature_bit(hash);
            // a repeated discriminant already has its bit set
            if sig & bit != 0 {
                split |= groups.iter().any(|&(h, start)| {
                    h == group_hash(hash) && self.nodes[start as usize].discriminant() == discrim
                });
            }
            groups.push((group_hash(hash), i as u32));
            sig |= bit;
            each_discriminant(&discrim);
            prev = Some(discrim);
        }
        if split {
            groups.clear();
            sig = ANY_DISCRIMINANT;
        }
        self.discrim_groups = groups;
        sig
    }
}
