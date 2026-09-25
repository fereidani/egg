//! Sharing-aware (DAG) greedy extraction.
//!
//! [`Extractor`](crate::Extractor) pays for a shared subterm once per
//! reference; this module prices a term by the e-classes it reaches, each
//! once, as after common-subexpression elimination.
//!
//! Each e-class keeps its cheapest known cost set (the node cost of every class
//! it reaches), recomputed when a child's total drops. Nodes that reach back
//! into their own class are skipped, keeping every choice acyclic. Past
//! [`MAX_SET_ENTRIES`] entries in total, extraction returns `None`.

use crate::no_std_prelude::*;

use crate::util::{HashMap, HashSet, set_remove};
use crate::{Analysis, EGraph, Id, Language, RecExpr};

/// Cost of an e-node itself, excluding its children: the DAG counterpart of
/// [`CostFunction`]. Use the tree cost function's weights so both extractors
/// optimize the same objective.
///
/// [`CostFunction`]: crate::CostFunction
pub trait NodeCost<L: Language> {
    /// Cost of `enode` itself, excluding its children.
    fn node_cost(&self, enode: &L) -> u64;
}

/// Bound on the total size of the cost sets, which can grow quadratically;
/// past it, extraction returns `None`.
const MAX_SET_ENTRIES: usize = 4_000_000;

/// The cheapest known choice for one e-class.
struct CostSet {
    /// Index of the chosen node within the class's node list.
    node_index: usize,
    /// Intrinsic cost of every class reachable through the choice, self
    /// included.
    costs: HashMap<Id, u64>,
    /// Saturating sum of `costs` values (cached).
    total: u64,
}

/// Extract a cheap term under DAG costing: every reachable e-class is priced
/// once, at its chosen node's intrinsic cost. Returns the DAG cost and the
/// extracted term (hash-consed, so shared classes appear once), or `None`
/// when the root has no finite extraction or the memory bound is exceeded.
///
/// This is a heuristic (a greedy fixpoint, then local improvement), so the
/// result can be a local minimum. Costs add with saturation, so `u64::MAX`
/// means "never choose".
///
/// Unlike [`Extractor`](crate::Extractor), which pays for a shared subterm
/// once per reference, this prices a term as it costs after
/// common-subexpression elimination.
///
/// ```
/// use egg::*;
///
/// struct Unit;
/// impl NodeCost<SymbolLang> for Unit {
///     fn node_cost(&self, _enode: &SymbolLang) -> u64 {
///         1
///     }
/// }
///
/// let mut egraph: EGraph<SymbolLang, ()> = EGraph::default();
/// let a = egraph.add(SymbolLang::leaf("a"));
/// let b = egraph.add(SymbolLang::leaf("b"));
/// let ab = egraph.add(SymbolLang::new("+", vec![a, b]));
/// let root = egraph.add(SymbolLang::new("*", vec![ab, ab]));
/// egraph.rebuild();
///
/// let (cost, best) = extract_dag(&egraph, root, &Unit).expect("root is extractable");
/// // four e-classes; tree costing would charge 7, one per expanded node
/// assert_eq!(cost, 4);
/// assert_eq!(best.as_ref().len(), 4);
/// assert_eq!(best.to_string(), "(* (+ a b) (+ a b))");
/// ```
pub fn extract_dag<L, A, C>(egraph: &EGraph<L, A>, root: Id, cost: &C) -> Option<(u64, RecExpr<L>)>
where
    L: Language,
    A: Analysis<L>,
    C: NodeCost<L>,
{
    let root = egraph.find(root);
    let sets = build_cost_sets(egraph, cost)?;
    let mut choices: HashMap<Id, usize> = sets
        .iter()
        .map(|(&cid, set)| (cid, set.node_index))
        .collect();
    if !choices.contains_key(&root) {
        return None;
    }
    // The fixpoint is greedy per class and misses a dearer node that reuses a
    // class the term already pays for, so improve against the whole term.
    refine_choices(egraph, cost, root, &mut choices);
    // `rebuild` needs acyclic choices, and a stored cost set can be stale
    choices_total(egraph, cost, root, &choices)?;
    let expr = rebuild(egraph, &choices, root)?;
    // the rebuilt expression holds each chosen class once, so this is its cost
    let total = expr
        .as_ref()
        .iter()
        .fold(0, |total: u64, n| total.saturating_add(cost.node_cost(n)));
    Some((total, expr))
}

/// DAG cost of the term `choices` select from `root`, or `None` if the
/// choices have a cycle or miss a class.
fn choices_total<L, A, C>(
    egraph: &EGraph<L, A>,
    cost: &C,
    root: Id,
    choices: &HashMap<Id, usize>,
) -> Option<u64>
where
    L: Language,
    A: Analysis<L>,
    C: NodeCost<L>,
{
    let mut total: u64 = 0;
    let mut done: HashSet<Id> = HashSet::default();
    let mut in_stack: HashSet<Id> = HashSet::default();
    let mut stack: Vec<(Id, bool)> = vec![(egraph.find(root), false)];
    while let Some((cid, ready)) = stack.pop() {
        if ready {
            set_remove(&mut in_stack, &cid);
            continue;
        }
        if done.contains(&cid) {
            continue;
        }
        // `in_stack` is a subset of `done`; cycles show up below
        in_stack.insert(cid);
        done.insert(cid);
        let node = egraph[cid].nodes.get(*choices.get(&cid)?)?;
        total = total.saturating_add(cost.node_cost(node));
        stack.push((cid, true));
        for &child in node.children() {
            let child = egraph.find(child);
            if in_stack.contains(&child) {
                return None;
            }
            if !done.contains(&child) {
                stack.push((child, false));
            }
        }
    }
    Some(total)
}

/// Hill-climbs `choices` under whole-term DAG pricing, keeping any node switch
/// that lowers the total. Terminates since the integer total strictly
/// decreases.
fn refine_choices<L, A, C>(
    egraph: &EGraph<L, A>,
    cost: &C,
    root: Id,
    choices: &mut HashMap<Id, usize>,
) where
    L: Language,
    A: Analysis<L>,
    C: NodeCost<L>,
{
    let Some(mut best_total) = choices_total(egraph, cost, root, choices) else {
        return;
    };
    loop {
        let mut improved = false;
        let reachable: Vec<Id> = collect_reachable(egraph, root, choices);
        for cid in reachable {
            let node_count = egraph[cid].nodes.len();
            let Some(mut current) = choices.get(&cid).copied() else {
                continue;
            };
            for candidate in 0..node_count {
                if candidate == current {
                    continue;
                }
                // A candidate whose children lack choices prices as None.
                choices.insert(cid, candidate);
                match choices_total(egraph, cost, root, choices) {
                    Some(total) if total < best_total => {
                        best_total = total;
                        current = candidate;
                        improved = true;
                    }
                    _ => {
                        choices.insert(cid, current);
                    }
                }
            }
        }
        if !improved {
            return;
        }
    }
}

/// The classes reachable from `root` through the current choices.
fn collect_reachable<L, A>(egraph: &EGraph<L, A>, root: Id, choices: &HashMap<Id, usize>) -> Vec<Id>
where
    L: Language,
    A: Analysis<L>,
{
    let mut out = Vec::new();
    let mut seen: HashSet<Id> = HashSet::default();
    let mut stack = vec![egraph.find(root)];
    while let Some(cid) = stack.pop() {
        if !seen.insert(cid) {
            continue;
        }
        out.push(cid);
        let Some(&idx) = choices.get(&cid) else {
            continue;
        };
        let Some(node) = egraph[cid].nodes.get(idx) else {
            continue;
        };
        for &child in node.children() {
            stack.push(egraph.find(child));
        }
    }
    out
}

/// Run the fixpoint that computes each class's cheapest cost set.
fn build_cost_sets<L, A, C>(egraph: &EGraph<L, A>, cost: &C) -> Option<HashMap<Id, CostSet>>
where
    L: Language,
    A: Analysis<L>,
    C: NodeCost<L>,
{
    let mut sets: HashMap<Id, CostSet> = HashMap::default();
    let mut entries: usize = 0;
    // seed with every class; changes propagate up through parents
    let mut pending: Vec<Id> = egraph.classes().map(|c| c.id).collect();
    let mut queued: HashSet<Id> = pending.iter().copied().collect();
    while let Some(cid) = pending.pop() {
        set_remove(&mut queued, &cid);
        let class = &egraph[cid];
        let Some(best) = best_choice(egraph, &sets, cost, cid, class.nodes.as_slice()) else {
            continue;
        };
        // count live entries: a replaced set stops counting
        let replaced = match sets.get(&cid) {
            Some(prev) => {
                if best.total >= prev.total {
                    continue;
                }
                prev.costs.len()
            }
            None => 0,
        };
        debug_assert!(entries >= replaced, "entries tracks the live set sizes");
        entries = entries - replaced + best.costs.len();
        if entries > MAX_SET_ENTRIES {
            return None;
        }
        sets.insert(cid, best);
        for parent in class.parents() {
            let parent = egraph.find(parent);
            if queued.insert(parent) {
                pending.push(parent);
            }
        }
    }
    Some(sets)
}

/// The cheapest usable node of `class`: children must already have cost sets
/// and must not reach back into `cid` (which would make the choice cyclic).
fn best_choice<L, A, C>(
    egraph: &EGraph<L, A>,
    sets: &HashMap<Id, CostSet>,
    cost: &C,
    cid: Id,
    nodes: &[L],
) -> Option<CostSet>
where
    L: Language,
    A: Analysis<L>,
    C: NodeCost<L>,
{
    let mut best: Option<CostSet> = None;
    'node: for (node_index, node) in nodes.iter().enumerate() {
        let mut costs: HashMap<Id, u64> = HashMap::default();
        for &child in node.children() {
            let child = egraph.find(child);
            let Some(child_set) = sets.get(&child) else {
                continue 'node;
            };
            if child_set.costs.contains_key(&cid) {
                continue 'node;
            }
            for (&k, &v) in &child_set.costs {
                costs.insert(k, v);
            }
        }
        costs.insert(cid, cost.node_cost(node));
        let total = costs
            .values()
            .fold(0, |total: u64, &c| total.saturating_add(c));
        if best.as_ref().is_none_or(|b| total < b.total) {
            best = Some(CostSet {
                node_index,
                costs,
                total,
            });
        }
    }
    best
}

/// Materialize the chosen nodes reachable from `root` as a hash-consed
/// `RecExpr` (children before parents; each class appears once).
fn rebuild<L, A>(
    egraph: &EGraph<L, A>,
    choices: &HashMap<Id, usize>,
    root: Id,
) -> Option<RecExpr<L>>
where
    L: Language,
    A: Analysis<L>,
{
    let mut expr = RecExpr::default();
    let mut placed: HashMap<Id, Id> = HashMap::default();
    // Iterative post-order. Loops forever on cyclic choices, which the caller
    // rules out with `choices_total`.
    let mut stack: Vec<(Id, bool)> = vec![(root, false)];
    while let Some((cid, ready)) = stack.pop() {
        let cid = egraph.find(cid);
        if placed.contains_key(&cid) {
            continue;
        }
        let node = egraph[cid].nodes.get(*choices.get(&cid)?)?;
        if ready {
            let rebuilt = node
                .clone()
                .map_children(|child| placed[&egraph.find(child)]);
            let placed_id = expr.add(rebuilt);
            placed.insert(cid, placed_id);
        } else {
            stack.push((cid, true));
            for &child in node.children() {
                stack.push((egraph.find(child), false));
            }
        }
    }
    debug_assert!(placed.contains_key(&egraph.find(root)), "root placed");
    Some(expr)
}
