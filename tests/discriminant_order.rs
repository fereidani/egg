//! E-matching must not depend on how a language's `Ord` arranges nodes.

use egg::*;

/// Derives `Ord` with the children before the operator, so sorting a class
/// can interleave operators.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ChildrenFirst {
    children: Vec<Id>,
    op: Symbol,
}

impl Language for ChildrenFirst {
    type Discriminant = Symbol;

    fn discriminant(&self) -> Self::Discriminant {
        self.op
    }

    fn matches(&self, other: &Self) -> bool {
        self.op == other.op && self.children.len() == other.children.len()
    }

    fn children(&self) -> &[Id] {
        &self.children
    }

    fn children_mut(&mut self) -> &mut [Id] {
        &mut self.children
    }
}

impl FromOp for ChildrenFirst {
    type Error = core::convert::Infallible;

    fn from_op(op: &str, children: Vec<Id>) -> Result<Self, Self::Error> {
        Ok(Self {
            op: op.into(),
            children,
        })
    }
}

fn node(op: &str, children: Vec<Id>) -> ChildrenFirst {
    ChildrenFirst {
        op: op.into(),
        children,
    }
}

#[test]
fn interleaved_discriminants_are_all_matched() {
    let mut egraph = EGraph::<ChildrenFirst, ()>::default();
    let a = egraph.add(node("a", vec![]));
    let b = egraph.add(node("b", vec![]));
    let c = egraph.add(node("c", vec![]));
    // sorted, the class reads f(a), g(b), f(c)
    let fa = egraph.add(node("f", vec![a]));
    let gb = egraph.add(node("g", vec![b]));
    let fc = egraph.add(node("f", vec![c]));
    egraph.union(fa, gb);
    egraph.union(fa, fc);
    egraph.rebuild();

    let pattern: Pattern<ChildrenFirst> = "(f ?x)".parse().unwrap();
    let matches = pattern.search(&egraph);
    let mut found: Vec<Id> = matches
        .iter()
        .flat_map(|m| m.substs.iter().map(|s| s["?x".parse().unwrap()]))
        .collect();
    found.sort();
    assert_eq!(found, vec![a, c]);

    let mut seen = vec![];
    let query = node("f", vec![Id::from(0)]);
    egraph[fa]
        .for_each_matching_node::<()>(&query, |n| {
            seen.push(n.children[0]);
            Ok(())
        })
        .unwrap();
    seen.sort();
    assert_eq!(seen, vec![a, c]);
}
