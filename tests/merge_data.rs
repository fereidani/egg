//! The flags [`merge_data`] derives decide whose parents get re-analyzed;
//! missing one silently drops facts.

use egg::{Analysis, DidMerge, EGraph, Id, SymbolLang, join_max, join_option, merge_data};

/// Keep a known value over an unknown one; two known values keep the first.
#[derive(Default)]
struct KeepFirst;

impl Analysis<SymbolLang> for KeepFirst {
    type Data = Option<i32>;

    fn make(_egraph: &mut EGraph<SymbolLang, Self>, _enode: &SymbolLang, _id: Id) -> Self::Data {
        None
    }

    fn join(&mut self, a: &Self::Data, b: &Self::Data) -> Self::Data {
        join_option(a, b, |a, _| *a)
    }
}

/// The larger value wins.
#[derive(Default)]
struct Max;

impl Analysis<SymbolLang> for Max {
    type Data = i32;

    fn make(_egraph: &mut EGraph<SymbolLang, Self>, _enode: &SymbolLang, _id: Id) -> Self::Data {
        0
    }

    fn join(&mut self, a: &Self::Data, b: &Self::Data) -> Self::Data {
        join_max(a, b)
    }
}

fn flags(did: DidMerge) -> (bool, bool) {
    (did.0, did.1)
}

#[test]
fn a_gaining_a_value_reports_only_a_moved() {
    let mut a = None;
    let did = merge_data::<SymbolLang, KeepFirst>(&mut KeepFirst, &mut a, Some(7));
    assert_eq!(a, Some(7), "the join kept the known value");
    assert_eq!(flags(did), (true, false), "only `a` changed");
}

#[test]
fn b_gaining_a_value_reports_only_b_moved() {
    // `a` is unchanged, but `b` gained the value, so its parents are revisited
    let mut a = Some(7);
    let did = merge_data::<SymbolLang, KeepFirst>(&mut KeepFirst, &mut a, None);
    assert_eq!(a, Some(7), "the join kept the known value");
    assert_eq!(flags(did), (false, true), "only `b` changed");
}

#[test]
fn equal_facts_report_nothing_moved() {
    let mut a = Some(7);
    let did = merge_data::<SymbolLang, KeepFirst>(&mut KeepFirst, &mut a, Some(7));
    assert_eq!(
        flags(did),
        (false, false),
        "joining a fact with itself is a no-op"
    );

    let mut a: Option<i32> = None;
    let did = merge_data::<SymbolLang, KeepFirst>(&mut KeepFirst, &mut a, None);
    assert_eq!(flags(did), (false, false), "two unknowns stay unknown");
}

#[test]
fn an_asymmetric_join_reports_the_losing_side() {
    let mut a = 3;
    let did = merge_data::<SymbolLang, Max>(&mut Max, &mut a, 5);
    assert_eq!(a, 5, "the larger value won");
    assert_eq!(flags(did), (true, false), "`a` was the one lifted");

    let mut a = 5;
    let did = merge_data::<SymbolLang, Max>(&mut Max, &mut a, 3);
    assert_eq!(a, 5, "the larger value won");
    assert_eq!(flags(did), (false, true), "`b` was the one lifted");
}
