//! [`BackoffScheduler`] must not panic once a ban count passes the word width.

use egg::{AstSize, BackoffScheduler, Extractor, Runner, StopReason, Symbol, SymbolLang, rewrite};

/// Enough iterations that the shift amount passes the word width.
const ITERS: usize = (usize::BITS as usize) + 16;

/// A runner kept going for [`ITERS`] iterations by a hook that adds one leaf
/// per iteration; banned rules alone would saturate at once.
fn ticking_runner() -> Runner<SymbolLang, ()> {
    Runner::default()
        .with_iter_limit(ITERS)
        .with_hook(|runner: &mut Runner<SymbolLang, ()>| {
            let name = format!("tick-{}", runner.iterations.len());
            runner.egraph.add(SymbolLang::leaf(name));
            // the runner searches right after the hooks
            runner.egraph.rebuild();
            Ok(())
        })
}

/// With a zero match limit and ban length, a rule is banned on every
/// iteration, so its ban count passes the word width after 64.
#[test]
fn ban_count_past_word_width_does_not_panic() {
    let rules = vec![
        rewrite!("commute-add"; "(+ ?a ?b)" => "(+ ?b ?a)"),
        rewrite!("commute-mul"; "(* ?a ?b)" => "(* ?b ?a)"),
    ];

    let scheduler = BackoffScheduler::default()
        .rule_match_limit("commute-add", 0)
        .rule_ban_length("commute-add", 0)
        .rule_match_limit("commute-mul", 0)
        .rule_ban_length("commute-mul", 0);

    let start = "(+ (* x y) z)".parse().unwrap();
    let runner = ticking_runner()
        .with_scheduler(scheduler)
        .with_expr(&start)
        .run(&rules);

    assert!(
        matches!(runner.stop_reason, Some(StopReason::IterationLimit(n)) if n == ITERS),
        "expected the iteration limit, got {:?}",
        runner.stop_reason
    );
    assert_eq!(runner.iterations.len(), ITERS);

    // every rule is banned on every iteration
    for (i, iter) in runner.iterations.iter().enumerate() {
        assert!(
            iter.applied.is_empty(),
            "iteration {i} applied {:?}, but every rule is banned",
            iter.applied
        );
    }

    // one leaf per iteration: the run itself was undisturbed
    assert!(
        runner.egraph.number_of_classes() >= ITERS,
        "expected at least one hook leaf per iteration, got {} classes",
        runner.egraph.number_of_classes()
    );
}

/// A rule banned forever must not stop the others.
#[test]
fn a_saturated_rule_does_not_stall_the_others() {
    let rules = vec![
        rewrite!("commute-add"; "(+ ?a ?b)" => "(+ ?b ?a)"),
        rewrite!("add-0"; "(+ ?a 0)" => "?a"),
    ];

    let scheduler = BackoffScheduler::default()
        .rule_match_limit("commute-add", 0)
        .rule_ban_length("commute-add", 0);

    let start = "(+ x 0)".parse().unwrap();
    let runner = ticking_runner()
        .with_scheduler(scheduler)
        .with_expr(&start)
        .run(&rules);

    assert_eq!(runner.iterations.len(), ITERS);

    let banned = Symbol::from("commute-add");
    for (i, iter) in runner.iterations.iter().enumerate() {
        assert!(
            !iter.applied.contains_key(&banned),
            "iteration {i} applied the permanently banned rule"
        );
    }

    // the unbanned rule kept applying
    assert!(
        runner
            .iterations
            .iter()
            .any(|iter| iter.applied.contains_key(&Symbol::from("add-0"))),
        "add-0 never applied"
    );
    let extractor = Extractor::new(&runner.egraph, AstSize);
    let (best_cost, best) = extractor.find_best(runner.roots[0]);
    assert_eq!(best_cost, 1);
    assert_eq!(best, "x".parse().unwrap());
}
