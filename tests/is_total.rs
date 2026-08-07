//! `Applier::is_total` says whether an applier fires on every match;
//! `get_pattern_ast` says what it builds when it does.

use egg::{Applier, ConditionalApplier, EGraph, Pattern, Rewrite, Runner, SymbolLang, rewrite};

/// A condition that never holds, so the wrapped pattern never fires.
struct Never;

impl egg::Condition<SymbolLang, ()> for Never {
    fn check(
        &self,
        _egraph: &mut EGraph<SymbolLang, ()>,
        _eclass: egg::Id,
        _subst: &egg::Subst,
    ) -> bool {
        false
    }

    fn vars(&self) -> Vec<egg::Var> {
        vec![]
    }
}

#[test]
fn a_bare_pattern_is_total() {
    let pattern: Pattern<SymbolLang> = "(+ ?a 0)".parse().expect("pattern parses");
    let applier: &dyn Applier<SymbolLang, ()> = &pattern;
    assert!(applier.is_total(), "a pattern fires on every match");
    assert!(
        applier.get_pattern_ast().is_some(),
        "and reports what it builds"
    );
}

#[test]
fn a_conditional_applier_reports_its_pattern_but_is_not_total() {
    let pattern: Pattern<SymbolLang> = "?a".parse().expect("pattern parses");
    let conditional = ConditionalApplier {
        condition: Never,
        applier: pattern,
    };
    let applier: &dyn Applier<SymbolLang, ()> = &conditional;
    // the ast is still reported
    assert!(
        applier.get_pattern_ast().is_some(),
        "the built pattern stays visible"
    );
    assert!(!applier.is_total(), "the condition can decline a match");
}

#[test]
fn a_declining_condition_really_does_block_the_rewrite() {
    // `is_total() == false` must correspond to real behaviour.
    let pattern: Pattern<SymbolLang> = "0".parse().expect("pattern parses");
    let guarded: Rewrite<SymbolLang, ()> = Rewrite::new(
        "never-fires",
        "(+ ?a 0)"
            .parse::<Pattern<SymbolLang>>()
            .expect("lhs parses"),
        ConditionalApplier {
            condition: Never,
            applier: pattern,
        },
    )
    .expect("rewrite builds");
    let plain: Rewrite<SymbolLang, ()> = rewrite!("add-0"; "(+ ?a 0)" => "?a");

    let expr = "(+ x 0)".parse().expect("expr parses");
    let guarded_run = Runner::<SymbolLang, (), ()>::default()
        .with_expr(&expr)
        .run(&[guarded]);
    let plain_run = Runner::<SymbolLang, (), ()>::default()
        .with_expr(&expr)
        .run(&[plain]);

    let root = guarded_run.roots[0];
    let x = guarded_run.egraph.lookup(SymbolLang::leaf("x"));
    assert_ne!(
        Some(guarded_run.egraph.find(root)),
        x.map(|x| guarded_run.egraph.find(x)),
        "the declined rewrite left the term alone"
    );

    let root = plain_run.roots[0];
    let x = plain_run.egraph.lookup(SymbolLang::leaf("x"));
    assert_eq!(
        Some(plain_run.egraph.find(root)),
        x.map(|x| plain_run.egraph.find(x)),
        "the unconditional one fired"
    );
}
