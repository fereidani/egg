use egg::{Id, Language, RecExpr, SymbolLang};

/// Builds `(f (f ... (f leaf)))` with `depth` applications of `f`.
fn deep_expr(depth: usize) -> RecExpr<SymbolLang> {
    let mut expr = RecExpr::default();
    let mut id = expr.add(SymbolLang::leaf("leaf"));
    for _ in 0..depth {
        id = expr.add(SymbolLang::new("f", vec![id]));
    }
    expr
}

#[test]
fn join_recexprs_grafts_children() {
    let a_plus_2: RecExpr<SymbolLang> = "(+ a 2)".parse().unwrap();
    let enode = SymbolLang::new("*", vec![Id::from(0), Id::from(0)]);
    let joined = enode.join_recexprs(|_id| &a_plus_2);
    assert_eq!(joined, "(* (+ a 2) (+ a 2))".parse().unwrap());
}

/// Joining must not recurse: the child expression can be arbitrarily deep.
#[test]
fn join_recexprs_handles_deep_children() {
    let deep = deep_expr(50_000);
    let enode = SymbolLang::new("g", vec![Id::from(0)]);
    let joined = enode.join_recexprs(|_id| &deep);
    assert_eq!(joined.len(), deep.len() + 1);
    assert_eq!(
        joined.last().unwrap().children(),
        &[Id::from(deep.len() - 1)]
    );
}

mod two_generics {
    use std::{
        fmt::{Debug, Display},
        hash::Hash,
        str::FromStr,
    };

    use egg::{Id, Language, RecExpr, define_language};

    define_language! {
        /// Two generics and a variant with both data and children.
        enum TwoGenerics<S, T> {
            Num(T),
            "+" = Add([Id; 2]),
            Call(S, Vec<Id>),
        }
        where
        S: Hash + Debug + Display + Clone + Eq + Ord + FromStr,
        T: Hash + Debug + Display + Clone + Eq + Ord + FromStr,
        <S as FromStr>::Err: Debug,
        <T as FromStr>::Err: Debug,
    }

    #[test]
    fn data_and_children_variant_accepts_several_generics() {
        let expr: RecExpr<TwoGenerics<egg::Symbol, i32>> = "(f (+ 1 2) 3)".parse().unwrap();
        let root = expr.last().unwrap();
        assert!(matches!(root, TwoGenerics::Call(op, _) if op.as_str() == "f"));
        assert_eq!(root.children().len(), 2);
        assert_eq!(
            expr[Id::from(2)],
            TwoGenerics::Add([Id::from(0), Id::from(1)])
        );
    }
}
