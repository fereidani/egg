//! Sharing-aware (DAG) extraction agrees with [`Extractor`] when nothing is
//! shared, and must prefer a dearer node that reuses a class the rest of the
//! term already pays for.

use egg::*;

/// Costs by op name, the same for both extractors: `opX` 3, `opY` 2, others 1.
struct ByName;

impl ByName {
    fn weight(op: &str) -> u64 {
        match op {
            "opX" => 3,
            "opY" => 2,
            _ => 1,
        }
    }
}

impl NodeCost<SymbolLang> for ByName {
    fn node_cost(&self, enode: &SymbolLang) -> u64 {
        Self::weight(enode.op.as_str())
    }
}

impl CostFunction<SymbolLang> for ByName {
    type Cost = u64;

    fn cost<C>(&mut self, enode: &SymbolLang, mut costs: C) -> Self::Cost
    where
        C: FnMut(Id) -> Self::Cost,
    {
        enode.fold(Self::weight(enode.op.as_str()), |sum, id| {
            sum.saturating_add(costs(id))
        })
    }
}

/// The ops of an extracted term, in the order they were placed.
fn ops(expr: &RecExpr<SymbolLang>) -> Vec<&str> {
    expr.as_ref().iter().map(|n| n.op.as_str()).collect()
}

/// `(* (+ a b) (+ a b))` has four e-classes but seven tree nodes.
#[test]
fn shared_subterm_is_priced_once() {
    let mut egraph: EGraph<SymbolLang, ()> = EGraph::default();
    let a = egraph.add(SymbolLang::leaf("a"));
    let b = egraph.add(SymbolLang::leaf("b"));
    let ab = egraph.add(SymbolLang::new("+", vec![a, b]));
    let root = egraph.add(SymbolLang::new("*", vec![ab, ab]));
    egraph.rebuild();

    let (cost, expr) = extract_dag(&egraph, root, &ByName).expect("root is extractable");
    assert_eq!(cost, 4, "a, b, + and * priced once each");
    assert_eq!(expr.as_ref().len(), 4, "hash-consed: (+ a b) stored once");
    assert_eq!(expr.to_string(), "(* (+ a b) (+ a b))");

    // The tree extractor agrees on the term but charges for `(+ a b)` twice.
    let (tree_cost, tree_expr) = Extractor::new(&egraph, ByName).find_best(root);
    assert_eq!(tree_cost, 7);
    assert_eq!(tree_expr.to_string(), expr.to_string());

    // children come before parents
    for (i, node) in expr.as_ref().iter().enumerate() {
        for &child in node.children() {
            assert!(usize::from(child) < i, "child before parent");
        }
    }
}

/// With nothing shared, DAG cost equals tree cost.
#[test]
fn unshared_term_costs_the_same_as_a_tree() {
    let mut egraph: EGraph<SymbolLang, ()> = EGraph::default();
    let a = egraph.add(SymbolLang::leaf("a"));
    let b = egraph.add(SymbolLang::leaf("b"));
    let ab = egraph.add(SymbolLang::new("+", vec![a, b]));
    let root = egraph.add(SymbolLang::new("neg", vec![ab]));
    egraph.rebuild();

    let (dag_cost, dag_expr) = extract_dag(&egraph, root, &ByName).expect("root is extractable");
    let (tree_cost, tree_expr) = Extractor::new(&egraph, ByName).find_best(root);
    assert_eq!(dag_cost, 4);
    assert_eq!(dag_cost, tree_cost);
    assert_eq!(dag_expr.to_string(), "(neg (+ a b))");
    assert_eq!(dag_expr.to_string(), tree_expr.to_string());
}

/// The root needs `(+ a b)` directly and through `(opX (+ a b))` or
/// `(opY (+ b c))`. Tree costing prefers `opY` (5 vs 6), DAG costing `opX`
/// (7 vs 8).
#[test]
fn dag_and_tree_extraction_pick_different_representatives() {
    let mut egraph: EGraph<SymbolLang, ()> = EGraph::default();
    let a = egraph.add(SymbolLang::leaf("a"));
    let b = egraph.add(SymbolLang::leaf("b"));
    let c = egraph.add(SymbolLang::leaf("c"));
    let ab = egraph.add(SymbolLang::new("+", vec![a, b]));
    let bc = egraph.add(SymbolLang::new("+", vec![b, c]));
    let x = egraph.add(SymbolLang::new("opX", vec![ab]));
    let y = egraph.add(SymbolLang::new("opY", vec![bc]));
    egraph.union(x, y);
    let root = egraph.add(SymbolLang::new("pair", vec![x, ab]));
    egraph.rebuild();

    let (cost, expr) = extract_dag(&egraph, root, &ByName).expect("root is extractable");
    assert_eq!(cost, 7, "pair + opX + (+ a b) + a + b, each once");
    let dag_ops = ops(&expr);
    assert!(dag_ops.contains(&"opX"), "reuses (+ a b): {dag_ops:?}");
    assert!(!dag_ops.contains(&"opY"), "no recompute: {dag_ops:?}");
    assert_eq!(
        dag_ops.iter().filter(|op| **op == "+").count(),
        1,
        "the shared subterm is emitted once: {dag_ops:?}"
    );

    // Same weights, opposite choice: tree costing cannot see the sharing.
    let extractor = Extractor::new(&egraph, ByName);
    assert_eq!(extractor.find_best_node(egraph.find(x)).op.as_str(), "opY");
    let (_, tree_expr) = extractor.find_best(root);
    let tree_ops = ops(&tree_expr);
    assert!(tree_ops.contains(&"opY"), "{tree_ops:?}");
}

/// A cyclic representative (`x = f(x)`) is skipped.
#[test]
fn self_referential_alternatives_are_skipped() {
    let mut egraph: EGraph<SymbolLang, ()> = EGraph::default();
    let x = egraph.add(SymbolLang::leaf("x"));
    let fx = egraph.add(SymbolLang::new("f", vec![x]));
    egraph.union(x, fx);
    egraph.rebuild();
    let root = egraph.find(x);

    let (cost, expr) = extract_dag(&egraph, root, &ByName).expect("root is extractable");
    assert_eq!(cost, 1);
    assert_eq!(expr.to_string(), "x");
}

// No test covers the `None` result:
//
//   - The memory bound trips on a chain of a few thousand nested nodes, but a
//     test would pin that limitation rather than a contract.
//   - A root with no cost set needs every node of its class to be cyclic,
//     which `add` and `union` cannot produce.

/// `u64::MAX` means "never choose": costs saturate instead of wrapping.
#[test]
fn saturated_costs_are_never_cheap() {
    struct Forbidding;
    impl NodeCost<SymbolLang> for Forbidding {
        fn node_cost(&self, enode: &SymbolLang) -> u64 {
            match enode.op.as_str() {
                "forbidden" => u64::MAX,
                "a" => 5,
                _ => 1,
            }
        }
    }

    let mut egraph: EGraph<SymbolLang, ()> = EGraph::default();
    let b = egraph.add(SymbolLang::leaf("b"));
    let forbidden = egraph.add(SymbolLang::new("forbidden", vec![b]));
    let a = egraph.add(SymbolLang::leaf("a"));
    egraph.union(a, forbidden);
    egraph.rebuild();

    let (cost, expr) = extract_dag(&egraph, a, &Forbidding).expect("root is extractable");
    assert_eq!(cost, 5);
    assert_eq!(expr.to_string(), "a");
}
