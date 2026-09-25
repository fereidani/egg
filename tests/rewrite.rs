//! Conditions and appliers.

use egg::*;

/// Substitution ids go stale when classes merge; the condition must compare
/// classes.
#[test]
fn condition_equal_compares_classes_not_ids() {
    let mut egraph = EGraph::<SymbolLang, ()>::default();
    let x = egraph.add(SymbolLang::leaf("x"));
    let y = egraph.add(SymbolLang::leaf("y"));
    egraph.rebuild();

    let mut subst = Subst::default();
    subst.insert("?a".parse().unwrap(), x);
    subst.insert("?b".parse().unwrap(), y);
    let same = ConditionEqual::<SymbolLang>::parse("?a", "?b");
    assert!(!same.check(&mut egraph, x, &subst));

    egraph.union(x, y);
    assert!(same.check(&mut egraph, x, &subst));
}
