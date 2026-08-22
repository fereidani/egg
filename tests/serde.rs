#![cfg(all(feature = "serde-1", feature = "serde_json"))]

use egg::{EGraph, SymbolLang};
use serde_json::{Value, json};

type Graph = EGraph<SymbolLang, ()>;

fn serialized_egraph() -> Value {
    let mut egraph = Graph::default();
    egraph.add_expr(&"(foo bar baz)".parse().unwrap());
    egraph.rebuild();
    serde_json::to_value(&egraph).unwrap()
}

#[test]
fn round_trips() {
    let egraph: Graph = serde_json::from_value(serialized_egraph()).unwrap();
    assert_eq!(egraph.number_of_classes(), 3);
    assert_eq!(egraph.total_size(), 3);
}

/// Lookups assume occupied slots are in bounds, so this must be rejected.
#[test]
fn rejects_out_of_range_slot() {
    let mut json = serialized_egraph();
    json["classes"]["index"][0] = json!(u32::MAX - 1);
    let err = serde_json::from_value::<Graph>(json).unwrap_err();
    assert!(err.to_string().contains("out of range"), "{err}");
}

#[test]
fn rejects_slot_naming_the_wrong_class() {
    let mut json = serialized_egraph();
    let last = json["classes"]["list"].as_array().unwrap().len() - 1;
    json["classes"]["index"][0] = json!(last);
    let err = serde_json::from_value::<Graph>(json).unwrap_err();
    assert!(err.to_string().contains("wrong e-class"), "{err}");
}

#[test]
fn rejects_unreachable_class() {
    let mut json = serialized_egraph();
    json["classes"]["index"][0] = json!(u32::MAX);
    let err = serde_json::from_value::<Graph>(json).unwrap_err();
    assert!(err.to_string().contains("missing a slot"), "{err}");
}
