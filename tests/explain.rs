//! Explanation regressions.

use std::{collections::HashMap, rc::Rc};

use egg::*;

type Graph = EGraph<SymbolLang, ()>;
type StepMemo = HashMap<*const TreeTerm<SymbolLang>, f64>;

/// Rewrite steps in the flattened `proof`, memoized per shared term. `f64`
/// because flattened proofs can be exponentially long.
fn flat_steps(proof: &[Rc<TreeTerm<SymbolLang>>], memo: &mut StepMemo) -> f64 {
    proof
        .iter()
        .map(|term| {
            if let Some(&steps) = memo.get(&Rc::as_ptr(term)) {
                return steps;
            }
            let own = f64::from(u8::from(
                term.forward_rule.is_some() || term.backward_rule.is_some(),
            ));
            let children: f64 = term
                .child_proofs
                .iter()
                .map(|child| flat_steps(child, memo))
                .sum();
            memo.insert(Rc::as_ptr(term), own + children);
            own + children
        })
        .sum()
}

fn proof_steps(egraph: &mut Graph, a: Id, b: Id) -> f64 {
    let proof = egraph.explain_id_equivalence(a, b);
    flat_steps(&proof.explanation_trees, &mut StepMemo::new())
}

/// Returns `pa` and `pb`, two rule steps apart and also congruent through
/// towers of 130 nested `(f t t)`: a proof of about 2^130 flat steps.
fn two_step_or_huge(optimize: bool) -> (Graph, Id, Id) {
    let mut egraph = Graph::default().with_explanations_enabled();
    if !optimize {
        egraph = egraph.without_explanation_length_optimization();
    }
    // padding keeps the congruence search from stopping early
    let pad = egraph.add_uncanonical(SymbolLang::leaf("pad"));
    for i in 0..2000 {
        let p = egraph.add_uncanonical(SymbolLang::leaf(format!("pad{i}")));
        egraph.union_trusted(p, pad, "pad");
    }
    let mut bases = vec![];
    let mut tops = vec![];
    for leaf in ["x", "y", "w"] {
        let mut id = egraph.add_uncanonical(SymbolLang::leaf(leaf));
        bases.push(id);
        for _ in 0..130 {
            id = egraph.add_uncanonical(SymbolLang::new("f", vec![id, id]));
        }
        tops.push(id);
    }
    let ka = egraph.add_uncanonical(SymbolLang::new("h", vec![tops[0]]));
    let kb = egraph.add_uncanonical(SymbolLang::new("h", vec![tops[1]]));
    let pa = egraph.add_uncanonical(SymbolLang::new("g", vec![ka]));
    let pb = egraph.add_uncanonical(SymbolLang::new("g", vec![kb]));
    let q = egraph.add_uncanonical(SymbolLang::leaf("q"));
    egraph.union_trusted(pa, q, "r1");
    egraph.union_trusted(q, pb, "r2");
    egraph.rebuild();
    egraph.union_trusted(bases[0], bases[1], "b1");
    egraph.union_trusted(bases[2], bases[1], "b2");
    egraph.rebuild();
    let kc = egraph.add_uncanonical(SymbolLang::new("h", vec![tops[2]]));
    egraph.add_uncanonical(SymbolLang::new("g", vec![kc]));
    egraph.rebuild();
    (egraph, pa, pb)
}

/// A saturated path cost must not make a huge proof look free.
#[test]
fn saturated_costs_do_not_make_long_proofs_look_free() {
    let (mut plain, pa, pb) = two_step_or_huge(false);
    let (mut optimized, _, _) = two_step_or_huge(true);
    let plain_steps = proof_steps(&mut plain, pb, pa);
    let optimized_steps = proof_steps(&mut optimized, pb, pa);
    assert_eq!(plain_steps, 2.0);
    assert!(
        optimized_steps <= plain_steps,
        "optimized proof has {optimized_steps:e} steps"
    );

    // a later direct rewrite must still be recorded and used
    optimized.union_trusted(pb, pa, "direct");
    optimized.rebuild();
    assert_eq!(proof_steps(&mut optimized, pb, pa), 1.0);
}

/// Unions at the far end of a long chain must not overflow the stack.
#[test]
fn unions_at_the_bottom_of_a_deep_explanation_tree() {
    let mut egraph = Graph::default().with_explanations_enabled();
    let mut prev = egraph.add_uncanonical(SymbolLang::leaf("a0"));
    let first = prev;
    for i in 1..100_000 {
        let next = egraph.add_uncanonical(SymbolLang::leaf(format!("a{i}")));
        egraph.union_trusted(next, prev, "chain");
        prev = next;
    }
    let fresh = egraph.add_uncanonical(SymbolLang::leaf("fresh"));
    egraph.union_trusted(prev, fresh, "last");
    egraph.rebuild();
    assert_eq!(egraph.find(first), egraph.find(fresh));
}
