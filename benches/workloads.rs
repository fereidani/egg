//! Workload benchmarks for egg, for use under `perf` or a timing wrapper:
//!
//!   cargo bench --bench workloads -- <workload|all> <reps>
//!
//! Each workload prints its wall times and a CHECK line of sizes or match
//! counts that must not change across optimizations. Limits are
//! deterministic, never time-based.

use egg::{rewrite as rw, *};
use std::time::{Duration, Instant};

const HUGE_TIME: Duration = Duration::from_secs(100_000);

// ---------------------------------------------------------------------------
// Math language (copied from tests/math.rs)
// ---------------------------------------------------------------------------
mod math {
    use super::*;
    use ordered_float::NotNan;

    pub type EGraph = egg::EGraph<Math, ConstantFold>;
    pub type Rewrite = egg::Rewrite<Math, ConstantFold>;
    pub type Constant = NotNan<f64>;

    define_language! {
        pub enum Math {
            "d" = Diff([Id; 2]),
            "i" = Integral([Id; 2]),

            "+" = Add([Id; 2]),
            "-" = Sub([Id; 2]),
            "*" = Mul([Id; 2]),
            "/" = Div([Id; 2]),
            "pow" = Pow([Id; 2]),
            "ln" = Ln(Id),
            "sqrt" = Sqrt(Id),

            "sin" = Sin(Id),
            "cos" = Cos(Id),

            Constant(Constant),
            Symbol(Symbol),
        }
    }

    #[derive(Default)]
    pub struct ConstantFold;
    impl Analysis<Math> for ConstantFold {
        type Data = Option<(Constant, PatternAst<Math>)>;

        fn make(egraph: &mut EGraph, enode: &Math, _id: Id) -> Self::Data {
            let x = |i: &Id| egraph[*i].data.as_ref().map(|d| d.0);
            Some(match enode {
                Math::Constant(c) => (*c, format!("{}", c).parse().unwrap()),
                Math::Add([a, b]) => (
                    x(a)? + x(b)?,
                    format!("(+ {} {})", x(a)?, x(b)?).parse().unwrap(),
                ),
                Math::Sub([a, b]) => (
                    x(a)? - x(b)?,
                    format!("(- {} {})", x(a)?, x(b)?).parse().unwrap(),
                ),
                Math::Mul([a, b]) => (
                    x(a)? * x(b)?,
                    format!("(* {} {})", x(a)?, x(b)?).parse().unwrap(),
                ),
                Math::Div([a, b]) if x(b) != Some(NotNan::new(0.0).unwrap()) => (
                    x(a)? / x(b)?,
                    format!("(/ {} {})", x(a)?, x(b)?).parse().unwrap(),
                ),
                _ => return None,
            })
        }

        fn join(&mut self, a: &Self::Data, b: &Self::Data) -> Self::Data {
            join_option(a, b, |a, b| {
                assert_eq!(a.0, b.0, "Merged non-equal constants");
                a.clone()
            })
        }

        fn modify(egraph: &mut EGraph, id: Id) {
            let data = egraph[id].data.clone();
            if let Some((c, pat)) = data {
                if egraph.are_explanations_enabled() {
                    egraph.union_instantiations(
                        &pat,
                        &format!("{}", c).parse().unwrap(),
                        &Default::default(),
                        "constant_fold".to_string(),
                    );
                } else {
                    let added = egraph.add(Math::Constant(c));
                    egraph.union(id, added);
                }
                egraph[id].nodes.retain(|n| n.is_leaf());
            }
        }
    }

    fn is_const_or_distinct_var(v: &str, w: &str) -> impl Fn(&mut EGraph, Id, &Subst) -> bool {
        let v = v.parse().unwrap();
        let w = w.parse().unwrap();
        move |egraph, _, subst| {
            egraph.find(subst[v]) != egraph.find(subst[w])
                && (egraph[subst[v]].data.is_some()
                    || egraph[subst[v]]
                        .nodes
                        .iter()
                        .any(|n| matches!(n, Math::Symbol(..))))
        }
    }

    fn is_const(var: &str) -> impl Fn(&mut EGraph, Id, &Subst) -> bool {
        let var = var.parse().unwrap();
        move |egraph, _, subst| egraph[subst[var]].data.is_some()
    }

    fn is_sym(var: &str) -> impl Fn(&mut EGraph, Id, &Subst) -> bool {
        let var = var.parse().unwrap();
        move |egraph, _, subst| {
            egraph[subst[var]]
                .nodes
                .iter()
                .any(|n| matches!(n, Math::Symbol(..)))
        }
    }

    fn is_not_zero(var: &str) -> impl Fn(&mut EGraph, Id, &Subst) -> bool {
        let var = var.parse().unwrap();
        move |egraph, _, subst| {
            if let Some(n) = &egraph[subst[var]].data {
                *(n.0) != 0.0
            } else {
                true
            }
        }
    }

    #[rustfmt::skip]
    pub fn rules() -> Vec<Rewrite> { vec![
        rw!("comm-add";  "(+ ?a ?b)"        => "(+ ?b ?a)"),
        rw!("comm-mul";  "(* ?a ?b)"        => "(* ?b ?a)"),
        rw!("assoc-add"; "(+ ?a (+ ?b ?c))" => "(+ (+ ?a ?b) ?c)"),
        rw!("assoc-mul"; "(* ?a (* ?b ?c))" => "(* (* ?a ?b) ?c)"),

        rw!("sub-canon"; "(- ?a ?b)" => "(+ ?a (* -1 ?b))"),
        rw!("div-canon"; "(/ ?a ?b)" => "(* ?a (pow ?b -1))" if is_not_zero("?b")),

        rw!("zero-add"; "(+ ?a 0)" => "?a"),
        rw!("zero-mul"; "(* ?a 0)" => "0"),
        rw!("one-mul";  "(* ?a 1)" => "?a"),

        rw!("add-zero"; "?a" => "(+ ?a 0)"),
        rw!("mul-one";  "?a" => "(* ?a 1)"),

        rw!("cancel-sub"; "(- ?a ?a)" => "0"),
        rw!("cancel-div"; "(/ ?a ?a)" => "1" if is_not_zero("?a")),

        rw!("distribute"; "(* ?a (+ ?b ?c))"        => "(+ (* ?a ?b) (* ?a ?c))"),
        rw!("factor"    ; "(+ (* ?a ?b) (* ?a ?c))" => "(* ?a (+ ?b ?c))"),

        rw!("pow-mul"; "(* (pow ?a ?b) (pow ?a ?c))" => "(pow ?a (+ ?b ?c))"),
        rw!("pow0"; "(pow ?x 0)" => "1" if is_not_zero("?x")),
        rw!("pow1"; "(pow ?x 1)" => "?x"),
        rw!("pow2"; "(pow ?x 2)" => "(* ?x ?x)"),
        rw!("pow-recip"; "(pow ?x -1)" => "(/ 1 ?x)" if is_not_zero("?x")),
        rw!("recip-mul-div"; "(* ?x (/ 1 ?x))" => "1" if is_not_zero("?x")),

        rw!("d-variable"; "(d ?x ?x)" => "1" if is_sym("?x")),
        rw!("d-constant"; "(d ?x ?c)" => "0" if is_sym("?x") if is_const_or_distinct_var("?c", "?x")),

        rw!("d-add"; "(d ?x (+ ?a ?b))" => "(+ (d ?x ?a) (d ?x ?b))"),
        rw!("d-mul"; "(d ?x (* ?a ?b))" => "(+ (* ?a (d ?x ?b)) (* ?b (d ?x ?a)))"),

        rw!("d-sin"; "(d ?x (sin ?x))" => "(cos ?x)"),
        rw!("d-cos"; "(d ?x (cos ?x))" => "(* -1 (sin ?x))"),

        rw!("d-ln"; "(d ?x (ln ?x))" => "(/ 1 ?x)" if is_not_zero("?x")),

        rw!("d-power";
            "(d ?x (pow ?f ?g))" =>
            "(* (pow ?f ?g)
                (+ (* (d ?x ?f)
                      (/ ?g ?f))
                   (* (d ?x ?g)
                      (ln ?f))))"
            if is_not_zero("?f")
            if is_not_zero("?g")
        ),

        rw!("i-one"; "(i 1 ?x)" => "?x"),
        rw!("i-power-const"; "(i (pow ?x ?c) ?x)" =>
            "(/ (pow ?x (+ ?c 1)) (+ ?c 1))" if is_const("?c")),
        rw!("i-cos"; "(i (cos ?x) ?x)" => "(sin ?x)"),
        rw!("i-sin"; "(i (sin ?x) ?x)" => "(* -1 (cos ?x))"),
        rw!("i-sum"; "(i (+ ?f ?g) ?x)" => "(+ (i ?f ?x) (i ?g ?x))"),
        rw!("i-dif"; "(i (- ?f ?g) ?x)" => "(- (i ?f ?x) (i ?g ?x))"),
        rw!("i-parts"; "(i (* ?a ?b) ?x)" =>
            "(- (* ?a (i ?b ?x)) (i (* (d ?x ?a) (i ?b ?x)) ?x))"),
    ]}

    pub const EXPRS: &[&str] = &[
        "(i (ln x) x)",
        "(i (+ x (cos x)) x)",
        "(i (* (cos x) x) x)",
        "(d x (+ 1 (* 2 x)))",
        "(d x (- (pow x 3) (* 7 (pow x 2))))",
        "(+ (* y (+ x y)) (- (+ x 2) (+ x x)))",
        "(/ 1 (- (/ (+ 1 (sqrt five)) 2) (/ (- 1 (sqrt five)) 2)))",
    ];

    pub const EXTRA_PATTERNS: &[&str] = &[
        "(+ ?a (+ ?b ?c))",
        "(+ (+ ?a ?b) ?c)",
        "(* ?a (* ?b ?c))",
        "(* (* ?a ?b) ?c)",
        "(+ ?a (* -1 ?b))",
        "(* ?a (pow ?b -1))",
        "(* ?a (+ ?b ?c))",
        "(pow ?a (+ ?b ?c))",
        "(+ (* ?a ?b) (* ?a ?c))",
        "(* (pow ?a ?b) (pow ?a ?c))",
        "(* ?x (/ 1 ?x))",
        "(d ?x (+ ?a ?b))",
        "(+ (d ?x ?a) (d ?x ?b))",
        "(d ?x (* ?a ?b))",
        "(+ (* ?a (d ?x ?b)) (* ?b (d ?x ?a)))",
        "(d ?x (sin ?x))",
        "(d ?x (cos ?x))",
        "(* -1 (sin ?x))",
        "(* -1 (cos ?x))",
        "(i (cos ?x) ?x)",
        "(i (sin ?x) ?x)",
        "(d ?x (ln ?x))",
        "(d ?x (pow ?f ?g))",
        "(* (pow ?f ?g) (+ (* (d ?x ?f) (/ ?g ?f)) (* (d ?x ?g) (ln ?f))))",
        "(i (pow ?x ?c) ?x)",
        "(/ (pow ?x (+ ?c 1)) (+ ?c 1))",
        "(i (+ ?f ?g) ?x)",
        "(i (- ?f ?g) ?x)",
        "(+ (i ?f ?x) (i ?g ?x))",
        "(- (i ?f ?x) (i ?g ?x))",
        "(i (* ?a ?b) ?x)",
        "(- (* ?a (i ?b ?x)) (i (* (d ?x ?a) (i ?b ?x)) ?x))",
    ];
}

// ---------------------------------------------------------------------------
// Lambda language (copied from tests/lambda.rs)
// ---------------------------------------------------------------------------
mod lambda {
    use super::*;
    use rustc_hash::FxHashSet as HashSet;

    define_language! {
        #[allow(clippy::enum_variant_names)]
        pub enum Lambda {
            Bool(bool),
            Num(i32),

            "var" = Var(Id),

            "+" = Add([Id; 2]),
            "=" = Eq([Id; 2]),

            "app" = App([Id; 2]),
            "lam" = Lambda([Id; 2]),
            "let" = Let([Id; 3]),
            "fix" = Fix([Id; 2]),

            "if" = If([Id; 3]),

            Symbol(egg::Symbol),
        }
    }

    impl Lambda {
        fn num(&self) -> Option<i32> {
            match self {
                Lambda::Num(n) => Some(*n),
                _ => None,
            }
        }
    }

    pub type EGraph = egg::EGraph<Lambda, LambdaAnalysis>;

    #[derive(Default)]
    pub struct LambdaAnalysis;

    #[derive(Debug, PartialEq)]
    pub struct Data {
        free: HashSet<Id>,
        constant: Option<(Lambda, PatternAst<Lambda>)>,
    }

    fn eval(egraph: &EGraph, enode: &Lambda) -> Option<(Lambda, PatternAst<Lambda>)> {
        let x = |i: &Id| egraph[*i].data.constant.as_ref().map(|c| &c.0);
        match enode {
            Lambda::Num(n) => Some((enode.clone(), format!("{}", n).parse().unwrap())),
            Lambda::Bool(b) => Some((enode.clone(), format!("{}", b).parse().unwrap())),
            Lambda::Add([a, b]) => Some((
                Lambda::Num(x(a)?.num()?.checked_add(x(b)?.num()?)?),
                format!("(+ {} {})", x(a)?, x(b)?).parse().unwrap(),
            )),
            Lambda::Eq([a, b]) => Some((
                Lambda::Bool(x(a)? == x(b)?),
                format!("(= {} {})", x(a)?, x(b)?).parse().unwrap(),
            )),
            _ => None,
        }
    }

    impl Analysis<Lambda> for LambdaAnalysis {
        type Data = Data;
        fn join(&mut self, a: &Data, b: &Data) -> Data {
            Data {
                free: a.free.intersection(&b.free).copied().collect(),
                constant: join_option(&a.constant, &b.constant, |a, b| {
                    assert_eq!(a.0, b.0, "Merged non-equal constants");
                    a.clone()
                }),
            }
        }

        fn make(egraph: &mut EGraph, enode: &Lambda, _id: Id) -> Data {
            let f = |i: &Id| egraph[*i].data.free.iter().cloned();
            let mut free = HashSet::default();
            match enode {
                Lambda::Var(v) => {
                    free.insert(*v);
                }
                Lambda::Let([v, a, b]) => {
                    free.extend(f(b));
                    free.remove(v);
                    free.extend(f(a));
                }
                Lambda::Lambda([v, a]) | Lambda::Fix([v, a]) => {
                    free.extend(f(a));
                    free.remove(v);
                }
                _ => enode.for_each(|c| free.extend(&egraph[c].data.free)),
            }
            let constant = eval(egraph, enode);
            Data { constant, free }
        }

        fn modify(egraph: &mut EGraph, id: Id) {
            if let Some(c) = egraph[id].data.constant.clone() {
                if egraph.are_explanations_enabled() {
                    egraph.union_instantiations(
                        &c.0.to_string().parse().unwrap(),
                        &c.1,
                        &Default::default(),
                        "analysis".to_string(),
                    );
                } else {
                    let const_id = egraph.add(c.0);
                    egraph.union(id, const_id);
                }
            }
        }
    }

    fn var(s: &str) -> Var {
        s.parse().unwrap()
    }

    fn is_not_same_var(v1: Var, v2: Var) -> impl Fn(&mut EGraph, Id, &Subst) -> bool {
        move |egraph, _, subst| egraph.find(subst[v1]) != egraph.find(subst[v2])
    }

    fn is_const(v: Var) -> impl Fn(&mut EGraph, Id, &Subst) -> bool {
        move |egraph, _, subst| egraph[subst[v]].data.constant.is_some()
    }

    pub fn rules() -> Vec<Rewrite<Lambda, LambdaAnalysis>> {
        vec![
            rw!("if-true";  "(if  true ?then ?else)" => "?then"),
            rw!("if-false"; "(if false ?then ?else)" => "?else"),
            rw!("if-elim"; "(if (= (var ?x) ?e) ?then ?else)" => "?else"
                if ConditionEqual::parse("(let ?x ?e ?then)", "(let ?x ?e ?else)")),
            rw!("add-comm";  "(+ ?a ?b)"        => "(+ ?b ?a)"),
            rw!("add-assoc"; "(+ (+ ?a ?b) ?c)" => "(+ ?a (+ ?b ?c))"),
            rw!("eq-comm";   "(= ?a ?b)"        => "(= ?b ?a)"),
            rw!("fix";      "(fix ?v ?e)"             => "(let ?v (fix ?v ?e) ?e)"),
            rw!("beta";     "(app (lam ?v ?body) ?e)" => "(let ?v ?e ?body)"),
            rw!("let-app";  "(let ?v ?e (app ?a ?b))" => "(app (let ?v ?e ?a) (let ?v ?e ?b))"),
            rw!("let-add";  "(let ?v ?e (+   ?a ?b))" => "(+   (let ?v ?e ?a) (let ?v ?e ?b))"),
            rw!("let-eq";   "(let ?v ?e (=   ?a ?b))" => "(=   (let ?v ?e ?a) (let ?v ?e ?b))"),
            rw!("let-const";
                "(let ?v ?e ?c)" => "?c" if is_const(var("?c"))),
            rw!("let-if";
                "(let ?v ?e (if ?cond ?then ?else))" =>
                "(if (let ?v ?e ?cond) (let ?v ?e ?then) (let ?v ?e ?else))"
            ),
            rw!("let-var-same"; "(let ?v1 ?e (var ?v1))" => "?e"),
            rw!("let-var-diff"; "(let ?v1 ?e (var ?v2))" => "(var ?v2)"
                if is_not_same_var(var("?v1"), var("?v2"))),
            rw!("let-lam-same"; "(let ?v1 ?e (lam ?v1 ?body))" => "(lam ?v1 ?body)"),
            rw!("let-lam-diff";
                "(let ?v1 ?e (lam ?v2 ?body))" =>
                { CaptureAvoid {
                    fresh: var("?fresh"), v2: var("?v2"), e: var("?e"),
                    if_not_free: "(lam ?v2 (let ?v1 ?e ?body))".parse().unwrap(),
                    if_free: "(lam ?fresh (let ?v1 ?e (let ?v2 (var ?fresh) ?body)))".parse().unwrap(),
                }}
                if is_not_same_var(var("?v1"), var("?v2"))),
        ]
    }

    struct CaptureAvoid {
        fresh: Var,
        v2: Var,
        e: Var,
        if_not_free: Pattern<Lambda>,
        if_free: Pattern<Lambda>,
    }

    impl Applier<Lambda, LambdaAnalysis> for CaptureAvoid {
        fn apply_one(
            &self,
            egraph: &mut EGraph,
            eclass: Id,
            subst: &Subst,
            searcher_ast: Option<&PatternAst<Lambda>>,
            rule_name: Symbol,
        ) -> Vec<Id> {
            let e = subst[self.e];
            let v2 = subst[self.v2];
            let v2_free_in_e = egraph[e].data.free.contains(&v2);
            if v2_free_in_e {
                let mut subst = subst.clone();
                let sym = Lambda::Symbol(format!("_{}", eclass).into());
                subst.insert(self.fresh, egraph.add(sym));
                self.if_free
                    .apply_one(egraph, eclass, &subst, searcher_ast, rule_name)
            } else {
                self.if_not_free
                    .apply_one(egraph, eclass, subst, searcher_ast, rule_name)
            }
        }
    }

    pub const FIB: &str = "(let fib (fix fib (lam n
        (if (= (var n) 0)
            0
        (if (= (var n) 1)
            1
        (+ (app (var fib)
                (+ (var n) -1))
            (app (var fib)
                (+ (var n) -2)))))))
        (app (var fib) 4))";

    pub const COMPOSE_MANY: &str = "(let compose (lam f (lam g (lam x (app (var f)
                                       (app (var g) (var x))))))
     (let add1 (lam y (+ (var y) 1))
     (app (app (var compose) (var add1))
          (app (app (var compose) (var add1))
               (app (app (var compose) (var add1))
                    (app (app (var compose) (var add1))
                         (app (app (var compose) (var add1))
                              (app (app (var compose) (var add1))
                                   (var add1)))))))))";
}

// ---------------------------------------------------------------------------
// Workloads: each returns a checksum that must stay stable across commits
// ---------------------------------------------------------------------------

fn math_saturate() -> u64 {
    let node_limit = 250_000;
    let mut runner = Runner::default()
        .with_scheduler(SimpleScheduler)
        .with_iter_limit(7)
        .with_node_limit(usize::MAX)
        .with_time_limit(HUGE_TIME)
        .with_hook(move |runner| {
            if runner.egraph.total_number_of_nodes() > node_limit {
                Err("stop".into())
            } else {
                Ok(())
            }
        });
    for expr in math::EXPRS {
        runner = runner.with_expr(&expr.parse().unwrap());
    }
    let runner = runner.run(&math::rules());
    (runner.egraph.total_number_of_nodes() * 31 + runner.egraph.number_of_classes()) as u64
}

fn math_diff_power() -> u64 {
    let runner: Runner<math::Math, math::ConstantFold> = Runner::default()
        .with_iter_limit(60)
        .with_node_limit(100_000)
        .with_time_limit(HUGE_TIME)
        .with_expr(&"(* x (- (* 3 x) 14))".parse().unwrap())
        .with_expr(&"(d x (- (pow x 3) (* 7 (pow x 2))))".parse().unwrap())
        .run(&math::rules());
    (runner.egraph.total_number_of_nodes() * 31 + runner.egraph.number_of_classes()) as u64
}

/// Only the saturation run with explanations enabled, without proof extraction.
fn math_explain_run() -> u64 {
    let start: RecExpr<math::Math> = "(d x (- (pow x 3) (* 7 (pow x 2))))".parse().unwrap();
    let goal: RecExpr<math::Math> = "(* x (- (* 3 x) 14))".parse().unwrap();
    let runner: Runner<math::Math, math::ConstantFold> = Runner::default()
        .with_explanations_enabled()
        .with_iter_limit(60)
        .with_node_limit(100_000)
        .with_time_limit(HUGE_TIME)
        .with_expr(&goal)
        .with_expr(&start)
        .run(&math::rules());
    (runner.egraph.total_number_of_nodes() * 31 + runner.egraph.number_of_classes()) as u64
}

fn math_explain() -> u64 {
    let start: RecExpr<math::Math> = "(d x (- (pow x 3) (* 7 (pow x 2))))".parse().unwrap();
    let goal: RecExpr<math::Math> = "(* x (- (* 3 x) 14))".parse().unwrap();
    let mut runner: Runner<math::Math, math::ConstantFold> = Runner::default()
        .with_explanations_enabled()
        .with_iter_limit(60)
        .with_node_limit(100_000)
        .with_time_limit(HUGE_TIME)
        .with_expr(&goal)
        .with_expr(&start)
        .run(&math::rules());
    let expl = runner
        .explain_equivalence(&start, &goal)
        .get_flat_strings()
        .len();
    (runner.egraph.total_number_of_nodes() * 31 + expl) as u64
}

fn lambda_fib() -> u64 {
    let runner: Runner<lambda::Lambda, lambda::LambdaAnalysis> = Runner::default()
        .with_iter_limit(60)
        .with_node_limit(500_000)
        .with_time_limit(HUGE_TIME)
        .with_expr(&lambda::FIB.parse().unwrap())
        .run(&lambda::rules());
    (runner.egraph.total_number_of_nodes() * 31 + runner.egraph.number_of_classes()) as u64
}

fn lambda_compose() -> u64 {
    let runner: Runner<lambda::Lambda, lambda::LambdaAnalysis> = Runner::default()
        .with_iter_limit(30)
        .with_node_limit(10_000)
        .with_time_limit(HUGE_TIME)
        .with_expr(&lambda::COMPOSE_MANY.parse().unwrap())
        .run(&lambda::rules());
    (runner.egraph.total_number_of_nodes() * 31 + runner.egraph.number_of_classes()) as u64
}

struct EMatch {
    egraph: math::EGraph,
    patterns: Vec<Pattern<math::Math>>,
}

fn ematch_setup() -> EMatch {
    let node_limit = 150_000;
    let mut runner = Runner::default()
        .with_scheduler(SimpleScheduler)
        .with_iter_limit(7)
        .with_node_limit(usize::MAX)
        .with_time_limit(HUGE_TIME)
        .with_hook(move |runner| {
            if runner.egraph.total_number_of_nodes() > node_limit {
                Err("stop".into())
            } else {
                Ok(())
            }
        });
    for expr in math::EXPRS {
        runner = runner.with_expr(&expr.parse().unwrap());
    }
    let runner = runner.run(&math::rules());

    let mut patterns: Vec<Pattern<math::Math>> = vec![];
    for rule in &math::rules() {
        if let Some(ast) = rule.searcher.get_pattern_ast() {
            patterns.push(ast.alpha_rename().into())
        }
        if let Some(ast) = rule.applier.get_pattern_ast() {
            patterns.push(ast.alpha_rename().into())
        }
    }
    for extra in math::EXTRA_PATTERNS {
        let p: Pattern<math::Math> = extra.parse().unwrap();
        patterns.push(p.ast.alpha_rename().into());
    }
    patterns.retain(|p| p.ast.len() > 1);
    patterns.sort_by_key(|p| p.to_string());
    patterns.dedup();
    patterns.sort_by_key(|p| p.ast.len());

    EMatch {
        egraph: runner.egraph,
        patterns,
    }
}

fn ematch_run(e: &EMatch) -> u64 {
    let mut total: u64 = 0;
    for pat in &e.patterns {
        let matches = pat.search(&e.egraph);
        total += matches.iter().map(|m| m.substs.len()).sum::<usize>() as u64;
    }
    total
}

// ---------------------------------------------------------------------------

fn measure(name: &str, reps: usize, mut f: impl FnMut() -> u64) {
    let mut times = Vec::with_capacity(reps);
    let mut check = 0;
    for _ in 0..reps {
        let start = Instant::now();
        let c = f();
        let t = start.elapsed().as_secs_f64() * 1e3;
        times.push(t);
        check = c;
        println!("RUN {name} {t:.3} ms");
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let min = times[0];
    let median = times[times.len() / 2];
    println!("CHECK {name} {check}");
    println!("STAT {name} min {min:.3} ms, median {median:.3} ms");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let which = args.get(1).map(|s| s.as_str()).unwrap_or("all");
    let reps: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(3);

    let all = which == "all";
    if all || which == "math-saturate" {
        measure("math-saturate", reps, math_saturate);
    }
    if all || which == "math-diff-power" {
        measure("math-diff-power", reps, math_diff_power);
    }
    if all || which == "math-explain-run" {
        measure("math-explain-run", reps, math_explain_run);
    }
    if all || which == "math-explain" {
        measure("math-explain", reps, math_explain);
    }
    if all || which == "lambda-fib" {
        measure("lambda-fib", reps, lambda_fib);
    }
    if all || which == "lambda-compose" {
        measure("lambda-compose", reps, lambda_compose);
    }
    if all || which == "ematch-math" {
        let e = ematch_setup();
        measure("ematch-math", reps, || ematch_run(&e));
    }
}
