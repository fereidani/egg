#![cfg(feature = "std")]
//! DOT output.

use egg::*;

/// Before a rebuild, edges must still point at the child's canonical class.
#[test]
fn edges_target_the_child_class() {
    let mut egraph = EGraph::<SymbolLang, ()>::default();
    let x = egraph.add(SymbolLang::leaf("x"));
    egraph.add(SymbolLang::new("f", vec![x]));
    let y = egraph.add(SymbolLang::leaf("y"));
    // `y` has more parents, so it becomes the canonical id of the union
    egraph.add(SymbolLang::new("g", vec![y]));
    egraph.add(SymbolLang::new("h", vec![y]));
    egraph.union(x, y);
    assert_eq!(egraph.find(x), y);

    let dot = egraph.dot().to_string();
    assert!(!dot.contains(&format!("-> {x}.0 ")), "{dot}");
    assert!(dot.contains(&format!("-> {y}.0 ")), "{dot}");
}
