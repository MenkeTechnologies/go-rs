//! A small escape analysis over the Go AST.
//!
//! Go's compiler allocates a slice's backing array on the stack when the slice
//! cannot outlive its function, and — since Go 1.25 — gives an `append` to such
//! a slice a 32-byte stack buffer of its own to grow from. The buffer is
//! visible in a program's output: the first `append` to a nil slice of `int`
//! yields `cap == 4` rather than `1`, and the capacities that follow are `4 4 4
//! 4 8 …` rather than `1 2 4 4 8 …`. Whether the buffer is used depends on the
//! escape analysis, so reproducing the capacity means reproducing the verdict.
//!
//! This is the flow-insensitive core of it: a graph whose nodes are local
//! variables plus two sinks, `HEAP` and `RET`, and whose edges say "the
//! value of this expression may end up in that variable". A variable escapes
//! when a path leads from it to a sink. What puts a value into a sink:
//!
//! * a store to a package-level variable, through a pointer or slice parameter,
//!   or into a map, channel or goroutine;
//! * an argument to `fmt` (an interface conversion), to a stdlib function that
//!   keeps its argument (`sort.Slice`), or to a function value that is not a
//!   declared function;
//! * `&x` — taking the address of the variable itself;
//! * a `return` (the value leaves for the caller; `RET`).
//!
//! A declared function is summarized once per parameter — does the argument
//! leak, does it flow to the result — and the summaries are iterated to a fixed
//! point, so `total(xs)`, `ret(xs)` and `first(xs)` do not make `xs` escape
//! while `keep(xs)` (which stores it in a global) does.
//!
//! It is deliberately an approximation of Go's, tuned against the verdicts the
//! reference toolchain gave on the shapes below; where it cannot tell it says
//! "escapes", which is the heap behavior go-rs had before it existed.

use crate::ast::*;
use std::collections::{HashMap, HashSet};

/// Sink: reachable from anywhere the program cannot track.
const HEAP: &str = "$heap";
/// Sink: the function's results, which leave for the caller.
const RET: &str = "$ret";

/// Standard-library packages none of whose functions keep a slice argument
/// (checked against `go` for `sort.Ints`, `strings.Join`, `slices.*`, `bytes.*`,
/// and `errors.Join`). A call into any other package leaks its arguments.
const NONLEAKING_PKGS: &[&str] = &[
    "strings", "bytes", "slices", "maps", "strconv", "math", "unicode", "utf8", "cmp", "bits",
    "sort", "errors",
];

/// Functions of those packages that do keep (convert to an interface, store).
const LEAKING_FUNCS: &[&str] = &[
    "sort.Slice",
    "sort.SliceStable",
    "sort.Sort",
    "sort.Stable",
    "sort.IsSorted",
    "sort.Reverse",
    "sort.SliceIsSorted",
    "errors.As",
    "errors.Is",
];

/// What a declared function does with each of its parameters (the receiver is
/// parameter 0 of a method).
#[derive(Clone, Default, PartialEq)]
struct Summary {
    leaks: Vec<bool>,
    to_ret: Vec<bool>,
}

/// The analysis of a whole program: one `Summary` per declared function.
pub struct Escape {
    /// Keyed by function name, or `Type.method`.
    sums: HashMap<String, Summary>,
    /// Method name → the keys of every method of that name.
    by_method: HashMap<String, Vec<String>>,
    /// Package aliases of the imports, so `sort.Ints` is not read as a method.
    packages: HashSet<String>,
    /// Result types of every declared function, by key.
    result_tys: HashMap<String, Vec<String>>,
    /// Names a function other than `main` reads or writes without declaring
    /// them: package-level variables. `main`'s own top level shares its scope
    /// with them here, so these are never treated as `main`'s locals.
    globals_used: HashSet<String>,
}

/// One function's flow graph, built by `Walker`.
struct Graph {
    edges: HashMap<String, HashSet<String>>,
}

impl Graph {
    fn reaches(&self, from: &str, to: &str) -> bool {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut stack = vec![from];
        while let Some(n) = stack.pop() {
            if n == to {
                return true;
            }
            if !seen.insert(n) {
                continue;
            }
            if let Some(next) = self.edges.get(n) {
                stack.extend(next.iter().map(String::as_str));
            }
        }
        false
    }

    fn escapes(&self, name: &str) -> bool {
        self.reaches(name, HEAP) || self.reaches(name, RET)
    }
}

impl Escape {
    /// Analyze `prog`: every declared function's summary, to a fixed point.
    pub fn analyze(prog: &Program) -> Escape {
        let packages = prog
            .imports
            .iter()
            .map(|p| p.rsplit('/').next().unwrap_or(p).to_string())
            .collect();
        let mut by_method: HashMap<String, Vec<String>> = HashMap::new();
        for f in &prog.funcs {
            if f.receiver.is_some() {
                by_method
                    .entry(f.name.clone())
                    .or_default()
                    .push(func_key(f));
            }
        }
        let mut esc = Escape {
            sums: prog
                .funcs
                .iter()
                .map(|f| {
                    let n = func_params(f).len();
                    (
                        func_key(f),
                        Summary {
                            leaks: vec![false; n],
                            to_ret: vec![false; n],
                        },
                    )
                })
                .collect(),
            by_method,
            packages,
            result_tys: prog
                .funcs
                .iter()
                .map(|f| (func_key(f), f.results.clone()))
                .collect(),
            globals_used: HashSet::new(),
        };
        // Monotone: a summary only ever gains `true`s, so a handful of rounds
        // settles mutual recursion.
        for _ in 0..8 {
            let mut changed = false;
            let mut used = HashSet::new();
            for f in &prog.funcs {
                let (graph, names, globals) =
                    esc.graph_of(&func_params(f), &f.result_names, &f.body);
                used.extend(globals);
                let sum = Summary {
                    leaks: names.iter().map(|n| graph.reaches(n, HEAP)).collect(),
                    to_ret: names.iter().map(|n| graph.reaches(n, RET)).collect(),
                };
                if esc.sums.get(&func_key(f)) != Some(&sum) {
                    esc.sums.insert(func_key(f), sum);
                    changed = true;
                }
            }
            esc.globals_used = used;
            if !changed {
                break;
            }
        }
        esc
    }

    /// The variables of a function — its parameters and its locals — whose
    /// value never reaches a sink. `main` passes `is_main` so that names other
    /// functions use are not counted as its locals.
    pub fn non_escaping(
        &self,
        params: &[Param],
        result_names: &[String],
        body: &[Stmt],
        is_main: bool,
    ) -> HashSet<String> {
        let (graph, _, _) = self.graph_of(params, result_names, body);
        let mut out = HashSet::new();
        for name in declared(params, result_names, body) {
            if graph.escapes(&name) {
                continue;
            }
            if is_main && self.globals_used.contains(&name) {
                continue;
            }
            out.insert(name);
        }
        out
    }

    /// Build the flow graph of one body. Also returns the parameter node names
    /// in order and the free (package-level) names it touched.
    fn graph_of(
        &self,
        params: &[Param],
        result_names: &[String],
        body: &[Stmt],
    ) -> (Graph, Vec<String>, HashSet<String>) {
        let mut w = Walker {
            esc: self,
            g: Graph {
                edges: HashMap::new(),
            },
            locals: declared(params, result_names, body),
            params: params.iter().map(|p| p.name.clone()).collect(),
            closure_depth: 0,
            globals: HashSet::new(),
            tys: params
                .iter()
                .map(|p| (p.name.clone(), p.ty.clone()))
                .collect(),
        };
        w.collect_types(body);
        for n in result_names.iter().filter(|n| !n.is_empty()) {
            w.flow(std::slice::from_ref(n), RET);
        }
        for s in body {
            w.stmt(s);
        }
        let names = params.iter().map(|p| p.name.clone()).collect();
        (w.g, names, w.globals)
    }
}

fn func_key(f: &Func) -> String {
    match &f.receiver {
        Some(r) => format!("{}.{}", r.ty.trim_start_matches('*'), f.name),
        None => f.name.clone(),
    }
}

/// The receiver (when there is one) followed by the parameters.
fn func_params(f: &Func) -> Vec<Param> {
    f.receiver.iter().chain(f.params.iter()).cloned().collect()
}

/// Every name a function declares: parameters, named results, `var` / `:=`
/// bindings, range and select variables, and those of nested literals.
fn declared(params: &[Param], result_names: &[String], body: &[Stmt]) -> HashSet<String> {
    fn stmts(body: &[Stmt], out: &mut HashSet<String>) {
        for s in body {
            stmt(s, out);
        }
    }
    fn stmt(s: &Stmt, out: &mut HashSet<String>) {
        match s {
            Stmt::Var { name, init, .. } => {
                out.insert(name.clone());
                if let Some(e) = init {
                    expr(e, out);
                }
            }
            Stmt::Short { names, values, .. } => {
                out.extend(names.iter().cloned());
                values.iter().for_each(|e| expr(e, out));
            }
            Stmt::Assign { target, value, .. } => {
                expr(target, out);
                expr(value, out);
            }
            Stmt::AssignMulti {
                targets, values, ..
            } => {
                targets.iter().chain(values).for_each(|e| expr(e, out));
            }
            Stmt::IncDec { target, .. } => expr(target, out),
            Stmt::ExprStmt(e) => expr(e, out),
            Stmt::Return(vs, _) => vs.iter().for_each(|e| expr(e, out)),
            Stmt::If {
                init,
                cond,
                then,
                els,
                ..
            } => {
                if let Some(i) = init {
                    stmt(i, out);
                }
                expr(cond, out);
                stmts(then, out);
                stmts(els, out);
            }
            Stmt::For {
                init,
                cond,
                post,
                body,
                ..
            } => {
                for i in [init, post].into_iter().flatten() {
                    stmt(i, out);
                }
                if let Some(c) = cond {
                    expr(c, out);
                }
                stmts(body, out);
            }
            Stmt::ForRange {
                key,
                val,
                iter,
                body,
                ..
            } => {
                out.extend(key.iter().chain(val.iter()).cloned());
                expr(iter, out);
                stmts(body, out);
            }
            Stmt::Go { call, .. } | Stmt::Defer { call, .. } => expr(call, out),
            Stmt::Send { chan, val, .. } => {
                expr(chan, out);
                expr(val, out);
            }
            Stmt::Select { cases, default, .. } => {
                for c in cases {
                    match &c.comm {
                        SelectComm::Recv {
                            bind,
                            ok_bind,
                            chan,
                            ..
                        } => {
                            out.extend(bind.iter().chain(ok_bind.iter()).cloned());
                            expr(chan, out);
                        }
                        SelectComm::Send { chan, val } => {
                            expr(chan, out);
                            expr(val, out);
                        }
                    }
                    stmts(&c.body, out);
                }
                if let Some(d) = default {
                    stmts(d, out);
                }
            }
            Stmt::Switch {
                init,
                tag,
                cases,
                default,
                ..
            } => {
                if let Some(i) = init {
                    stmt(i, out);
                }
                if let Some(t) = tag {
                    expr(t, out);
                }
                for c in cases {
                    c.exprs.iter().for_each(|e| expr(e, out));
                    stmts(&c.body, out);
                }
                if let Some(d) = default {
                    stmts(d, out);
                }
            }
            Stmt::TypeSwitch {
                init,
                bind,
                expr: e,
                cases,
                default,
                ..
            } => {
                if let Some(i) = init {
                    stmt(i, out);
                }
                out.extend(bind.iter().cloned());
                expr(e, out);
                for c in cases {
                    stmts(&c.body, out);
                }
                if let Some(d) = default {
                    stmts(d, out);
                }
            }
            Stmt::Block(b) => stmts(b, out),
            Stmt::Fallthrough(_)
            | Stmt::Goto(..)
            | Stmt::Label(..)
            | Stmt::Break(..)
            | Stmt::Continue(..) => {}
        }
    }
    fn expr(e: &Expr, out: &mut HashSet<String>) {
        match e {
            Expr::FuncLit {
                params,
                result_names,
                body,
                ..
            } => {
                out.extend(params.iter().map(|p| p.name.clone()));
                out.extend(result_names.iter().filter(|n| !n.is_empty()).cloned());
                stmts(body, out);
            }
            Expr::Unary { rhs, .. } => expr(rhs, out),
            Expr::Binary { lhs, rhs, .. } => {
                expr(lhs, out);
                expr(rhs, out);
            }
            Expr::Call { func, args, .. } => {
                expr(func, out);
                args.iter().for_each(|a| expr(a, out));
            }
            Expr::Selector { recv, .. } => expr(recv, out),
            Expr::Index { recv, index } => {
                expr(recv, out);
                expr(index, out);
            }
            Expr::Slice {
                recv,
                low,
                high,
                max,
            } => {
                expr(recv, out);
                for b in [low, high, max].into_iter().flatten() {
                    expr(b, out);
                }
            }
            Expr::SliceLit { elems, .. } => elems.iter().for_each(|x| expr(x, out)),
            Expr::MapLit { pairs, .. } => pairs.iter().for_each(|(k, v)| {
                expr(k, out);
                expr(v, out);
            }),
            Expr::StructLit { fields, .. } => fields.iter().for_each(|(_, v)| expr(v, out)),
            Expr::Make {
                len,
                cap,
                elem_zero,
                ..
            } => {
                for b in [len, cap].into_iter().flatten() {
                    expr(b, out);
                }
                expr(elem_zero, out);
            }
            Expr::MakeChan { cap: Some(c), .. } => expr(c, out),
            Expr::Recv { chan } => expr(chan, out),
            Expr::TypeAssert { expr: e, .. } => expr(e, out),
            _ => {}
        }
    }
    let mut out: HashSet<String> = params.iter().map(|p| p.name.clone()).collect();
    out.extend(result_names.iter().filter(|n| !n.is_empty()).cloned());
    stmts(body, &mut out);
    out.remove("_");
    out
}

struct Walker<'a> {
    esc: &'a Escape,
    g: Graph,
    locals: HashSet<String>,
    params: HashSet<String>,
    /// How many function literals deep the walk is: a `return` inside one is
    /// not this function's result.
    closure_depth: usize,
    /// Free names touched: package-level variables.
    globals: HashSet<String>,
    /// Declared or evident types of locals, as written (`[]int`, `map[K]V`).
    tys: HashMap<String, String>,
}

impl Walker<'_> {
    /// Record that the values of `from` may end up in `to`.
    fn flow(&mut self, from: &[String], to: &str) {
        for f in from {
            self.g
                .edges
                .entry(f.clone())
                .or_default()
                .insert(to.to_string());
        }
    }

    fn leak(&mut self, from: &[String]) {
        self.flow(from, HEAP);
    }

    /// The node an assignment target stores into: the variable itself for a
    /// local, `HEAP` for a package-level name or a store through a
    /// parameter (the pointee belongs to the caller).
    fn target(&mut self, t: &Expr) -> String {
        match t {
            Expr::Ident(n) if n == "_" => "$discard".to_string(),
            Expr::Ident(n) => {
                if self.locals.contains(n) {
                    n.clone()
                } else {
                    self.globals.insert(n.clone());
                    HEAP.to_string()
                }
            }
            Expr::Index { recv, index } => {
                self.eval(index);
                self.stored_into(recv)
            }
            Expr::Selector { recv, .. } => self.stored_into(recv),
            Expr::Slice { recv, .. } => self.stored_into(recv),
            Expr::Unary {
                op: UnOp::Deref, ..
            } => HEAP.to_string(),
            other => {
                self.eval(other);
                HEAP.to_string()
            }
        }
    }

    /// The node holding the container `e` an element or field is stored into.
    fn stored_into(&mut self, e: &Expr) -> String {
        // A map's buckets live on the heap whatever the map variable does.
        if self.type_of(e).is_some_and(|t| t.starts_with("map[")) {
            self.eval(e);
            return HEAP.to_string();
        }
        match e {
            Expr::Ident(n) if self.locals.contains(n) && !self.params.contains(n) => n.clone(),
            Expr::Ident(n) if self.params.contains(n) => HEAP.to_string(),
            Expr::Ident(n) => {
                self.globals.insert(n.clone());
                HEAP.to_string()
            }
            Expr::Index { recv, index } => {
                self.eval(index);
                self.stored_into(recv)
            }
            Expr::Selector { recv, .. } => self.stored_into(recv),
            _ => {
                self.eval(e);
                HEAP.to_string()
            }
        }
    }

    /// The best-known written type of an expression, when evident.
    fn type_of(&self, e: &Expr) -> Option<String> {
        match e {
            Expr::Ident(n) => self.tys.get(n).cloned(),
            Expr::SliceLit {
                elem_ty, array_len, ..
            } => Some(match array_len {
                Some(n) => format!("[{n}]{elem_ty}"),
                None => format!("[]{elem_ty}"),
            }),
            Expr::MapLit { key_ty, val_ty, .. } => Some(format!("map[{key_ty}]{val_ty}")),
            Expr::Make {
                is_map, elem_ty, ..
            } => Some(if *is_map {
                elem_ty.clone()
            } else {
                format!("[]{elem_ty}")
            }),
            Expr::Slice { recv, .. } => {
                let t = self.type_of(recv)?;
                match array_elem_ty(&t) {
                    Some(elem) => Some(format!("[]{elem}")),
                    None => Some(t),
                }
            }
            Expr::Call { func, args, .. } => match &**func {
                Expr::Ident(n) if n == "append" => args.first().and_then(|a| self.type_of(a)),
                Expr::Ident(n) if n.starts_with("[]") || n.starts_with("map[") => Some(n.clone()),
                Expr::Ident(n) => self.esc.result_tys.get(n).and_then(|r| r.first().cloned()),
                _ => None,
            },
            _ => None,
        }
    }

    /// Record the evident type of every declaration in `body`, in source order.
    fn collect_types(&mut self, body: &[Stmt]) {
        for s in body {
            match s {
                Stmt::Var {
                    name, ty: Some(t), ..
                } => {
                    self.tys.insert(name.clone(), t.clone());
                }
                Stmt::Var {
                    name,
                    ty: None,
                    init: Some(e),
                    ..
                } => {
                    if let Some(t) = self.type_of(e) {
                        self.tys.insert(name.clone(), t);
                    }
                }
                Stmt::Short { names, values, .. } if names.len() == values.len() => {
                    for (n, v) in names.iter().zip(values) {
                        if let Some(t) = self.type_of(v) {
                            self.tys.insert(n.clone(), t);
                        }
                    }
                }
                Stmt::If {
                    init, then, els, ..
                } => {
                    if let Some(i) = init {
                        self.collect_types(std::slice::from_ref(&**i));
                    }
                    self.collect_types(then);
                    self.collect_types(els);
                }
                Stmt::For {
                    init, post, body, ..
                } => {
                    for i in [init, post].into_iter().flatten() {
                        self.collect_types(std::slice::from_ref(&**i));
                    }
                    self.collect_types(body);
                }
                Stmt::ForRange { body, .. } | Stmt::Block(body) => self.collect_types(body),
                Stmt::Switch { cases, default, .. } => {
                    for c in cases {
                        self.collect_types(&c.body);
                    }
                    if let Some(d) = default {
                        self.collect_types(d);
                    }
                }
                Stmt::TypeSwitch { cases, default, .. } => {
                    for c in cases {
                        self.collect_types(&c.body);
                    }
                    if let Some(d) = default {
                        self.collect_types(d);
                    }
                }
                Stmt::Select { cases, default, .. } => {
                    for c in cases {
                        self.collect_types(&c.body);
                    }
                    if let Some(d) = default {
                        self.collect_types(d);
                    }
                }
                _ => {}
            }
        }
    }

    fn assign(&mut self, target: &Expr, srcs: Vec<String>) {
        let to = self.target(target);
        self.flow(&srcs, &to);
    }

    fn stmts(&mut self, body: &[Stmt]) {
        for s in body {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Var { name, init, .. } => {
                if let Some(e) = init {
                    let srcs = self.eval(e);
                    self.flow(&srcs, name);
                }
            }
            Stmt::Short { names, values, .. } => {
                if names.len() == values.len() {
                    for (n, v) in names.iter().zip(values) {
                        let srcs = self.eval(v);
                        self.flow(&srcs, n);
                    }
                } else {
                    let srcs: Vec<String> = values.iter().flat_map(|v| self.eval(v)).collect();
                    for n in names {
                        self.flow(&srcs, n);
                    }
                }
            }
            Stmt::Assign { target, value, .. } => {
                let srcs = self.eval(value);
                self.assign(target, srcs);
            }
            Stmt::AssignMulti {
                targets, values, ..
            } => {
                let all: Vec<Vec<String>> = values.iter().map(|v| self.eval(v)).collect();
                if targets.len() == values.len() {
                    for (t, srcs) in targets.iter().zip(all) {
                        self.assign(t, srcs);
                    }
                } else {
                    let srcs: Vec<String> = all.into_iter().flatten().collect();
                    for t in targets {
                        self.assign(t, srcs.clone());
                    }
                }
            }
            Stmt::IncDec { target, .. } => {
                self.eval(target);
            }
            Stmt::ExprStmt(e) => {
                self.eval(e);
            }
            Stmt::Return(vs, _) => {
                let sink = if self.closure_depth > 0 { HEAP } else { RET };
                for v in vs {
                    let srcs = self.eval(v);
                    self.flow(&srcs, sink);
                }
            }
            Stmt::If {
                init,
                cond,
                then,
                els,
                ..
            } => {
                if let Some(i) = init {
                    self.stmt(i);
                }
                self.eval(cond);
                self.stmts(then);
                self.stmts(els);
            }
            Stmt::For {
                init,
                cond,
                post,
                body,
                ..
            } => {
                for i in [init, post].into_iter().flatten() {
                    self.stmt(i);
                }
                if let Some(c) = cond {
                    self.eval(c);
                }
                self.stmts(body);
            }
            Stmt::ForRange {
                key,
                val,
                iter,
                body,
                define,
                ..
            } => {
                let srcs = self.eval(iter);
                let scalar = self.type_of(iter).is_some_and(|t| scalar_elems(&t));
                for n in val.iter().chain(key.iter()) {
                    if !scalar && (*define || self.locals.contains(n)) {
                        self.flow(&srcs, n);
                    }
                }
                self.stmts(body);
            }
            Stmt::Go { call, .. } => self.call(call, true),
            Stmt::Defer { call, .. } => self.call(call, false),
            Stmt::Send { chan, val, .. } => {
                self.eval(chan);
                let srcs = self.eval(val);
                self.leak(&srcs);
            }
            Stmt::Select { cases, default, .. } => {
                for c in cases {
                    match &c.comm {
                        SelectComm::Recv { chan, .. } => {
                            self.eval(chan);
                        }
                        SelectComm::Send { chan, val } => {
                            self.eval(chan);
                            let srcs = self.eval(val);
                            self.leak(&srcs);
                        }
                    }
                    self.stmts(&c.body);
                }
                if let Some(d) = default {
                    self.stmts(d);
                }
            }
            Stmt::Switch {
                init,
                tag,
                cases,
                default,
                ..
            } => {
                if let Some(i) = init {
                    self.stmt(i);
                }
                if let Some(t) = tag {
                    self.eval(t);
                }
                for c in cases {
                    c.exprs.iter().for_each(|e| {
                        self.eval(e);
                    });
                    self.stmts(&c.body);
                }
                if let Some(d) = default {
                    self.stmts(d);
                }
            }
            Stmt::TypeSwitch {
                init,
                bind,
                expr,
                cases,
                default,
                ..
            } => {
                if let Some(i) = init {
                    self.stmt(i);
                }
                let srcs = self.eval(expr);
                if let Some(b) = bind {
                    self.flow(&srcs, b);
                }
                for c in cases {
                    self.stmts(&c.body);
                }
                if let Some(d) = default {
                    self.stmts(d);
                }
            }
            Stmt::Block(b) => self.stmts(b),
            Stmt::Fallthrough(_)
            | Stmt::Goto(..)
            | Stmt::Label(..)
            | Stmt::Break(..)
            | Stmt::Continue(..) => {}
        }
    }

    /// A `go` / `defer` statement's call. A goroutine outlives the frame, so
    /// everything it was handed — arguments and a literal's captures — leaks.
    fn call(&mut self, call: &Expr, go: bool) {
        let srcs = self.eval(call);
        if go {
            self.leak(&srcs);
            if let Expr::Call { func, args, .. } = call {
                let f = self.eval(func);
                self.leak(&f);
                for a in args {
                    let s = self.eval(a);
                    self.leak(&s);
                }
            }
        }
    }

    /// The variables whose values `e` may carry (for assignment and for
    /// arguments), with the effects of every call inside it recorded.
    fn eval(&mut self, e: &Expr) -> Vec<String> {
        match e {
            Expr::Ident(n) => {
                if self.locals.contains(n) {
                    vec![n.clone()]
                } else {
                    if !matches!(n.as_str(), "nil" | "true" | "false" | "iota" | "_") {
                        self.globals.insert(n.clone());
                    }
                    Vec::new()
                }
            }
            Expr::Int(_) | Expr::Float(..) | Expr::Str(_) | Expr::Bool(_) => Vec::new(),
            Expr::Unary { op, rhs } => {
                let srcs = self.eval(rhs);
                match op {
                    // The address of a variable: the variable itself escapes
                    // (Go's analysis moves it to the heap conservatively).
                    UnOp::Addr => {
                        if matches!(**rhs, Expr::Ident(_)) {
                            self.leak(&srcs);
                        }
                        srcs
                    }
                    UnOp::Deref => srcs,
                    _ => Vec::new(),
                }
            }
            Expr::Binary { lhs, rhs, .. } => {
                self.eval(lhs);
                self.eval(rhs);
                Vec::new()
            }
            Expr::Selector { recv, .. } => {
                if matches!(&**recv, Expr::Ident(p) if self.esc.packages.contains(p) && !self.locals.contains(p))
                {
                    return Vec::new();
                }
                self.eval(recv)
            }
            Expr::Index { recv, index } => {
                self.eval(index);
                let srcs = self.eval(recv);
                // An element of a slice of numbers or strings carries no
                // backing array.
                match self.type_of(recv) {
                    Some(t) if scalar_elems(&t) => Vec::new(),
                    _ => srcs,
                }
            }
            Expr::Slice {
                recv,
                low,
                high,
                max,
            } => {
                for b in [low, high, max].into_iter().flatten() {
                    self.eval(b);
                }
                self.eval(recv)
            }
            Expr::SliceLit { elems, .. } => elems.iter().flat_map(|x| self.eval(x)).collect(),
            Expr::MapLit { pairs, .. } => pairs
                .iter()
                .flat_map(|(k, v)| {
                    let mut s = self.eval(k);
                    s.extend(self.eval(v));
                    s
                })
                .collect(),
            Expr::StructLit { fields, .. } => {
                fields.iter().flat_map(|(_, v)| self.eval(v)).collect()
            }
            Expr::Make { len, cap, .. } => {
                for b in [len, cap].into_iter().flatten() {
                    self.eval(b);
                }
                Vec::new()
            }
            Expr::MakeChan { cap, .. } => {
                if let Some(c) = cap {
                    self.eval(c);
                }
                Vec::new()
            }
            Expr::Recv { chan } => {
                self.eval(chan);
                Vec::new()
            }
            Expr::TypeAssert { expr, .. } => self.eval(expr),
            Expr::Instantiate { .. } => Vec::new(),
            Expr::FuncLit {
                params,
                result_names,
                body,
                ..
            } => {
                // The literal's own names are locals of this function's graph.
                let added: Vec<String> = declared(params, result_names, body)
                    .into_iter()
                    .filter(|n| self.locals.insert(n.clone()))
                    .collect();
                // Everything the body touches that is not its own: the
                // captured variables the closure value carries.
                let own = declared(params, result_names, body);
                let mut captured = HashSet::new();
                collect_idents(body, &mut captured);
                let captured: Vec<String> = captured
                    .into_iter()
                    .filter(|n| !own.contains(n) && self.locals.contains(n))
                    .collect();
                self.closure_depth += 1;
                self.stmts(body);
                self.closure_depth -= 1;
                for n in added {
                    self.locals.remove(&n);
                }
                captured
            }
            Expr::Call {
                func, args, spread, ..
            } => self.eval_call(func, args, *spread),
        }
    }

    fn eval_call(&mut self, func: &Expr, args: &[Expr], spread: bool) -> Vec<String> {
        let arg_srcs: Vec<Vec<String>> = args.iter().map(|a| self.eval(a)).collect();
        let all = || -> Vec<String> { arg_srcs.iter().flatten().cloned().collect() };
        match func {
            Expr::Ident(name) if !self.locals.contains(name) => {
                match name.as_str() {
                    "len" | "cap" | "copy" | "delete" | "close" | "clear" | "min" | "max"
                    | "print" | "println" | "recover" | "new" | "make" | "real" | "imag"
                    | "complex" => return Vec::new(),
                    "append" => return all(),
                    "panic" => {
                        self.leak(&all());
                        return Vec::new();
                    }
                    _ => {}
                }
                if let Some(sum) = self.esc.sums.get(name) {
                    // A vendored package function: its body is Go source, but
                    // the verdict that matters is the package's, as for a
                    // selector call.
                    let forced = LEAKING_FUNCS.contains(&name.as_str());
                    let keeps = match name.split_once('.') {
                        Some((pkg, _)) => !NONLEAKING_PKGS.contains(&pkg),
                        None => true,
                    };
                    if forced {
                        self.leak(&all());
                    }
                    let mut result = Vec::new();
                    for (i, srcs) in arg_srcs.iter().enumerate() {
                        let idx = i.min(sum.leaks.len().saturating_sub(1));
                        if keeps && sum.leaks.get(idx).copied().unwrap_or(false) {
                            self.leak(srcs);
                        }
                        if sum.to_ret.get(idx).copied().unwrap_or(false) {
                            result.extend(srcs.iter().cloned());
                        }
                    }
                    return result;
                }
                // A conversion `T(x)` or a call of an unresolved name: the
                // value passes through, and nothing is known to keep it.
                if is_conversion_name(name) {
                    return all();
                }
                self.leak(&all());
                all()
            }
            Expr::Selector { recv, field } => {
                if let Expr::Ident(p) = &**recv {
                    if self.esc.packages.contains(p) && !self.locals.contains(p) {
                        let qualified = format!("{p}.{field}");
                        let keeps = !NONLEAKING_PKGS.contains(&p.as_str())
                            || LEAKING_FUNCS.contains(&qualified.as_str());
                        if keeps {
                            self.leak(&all());
                            return all();
                        }
                        return Vec::new();
                    }
                }
                // A method call: the receiver is parameter 0.
                let recv_srcs = self.eval(recv);
                let keys = self.esc.by_method.get(field.as_str());
                let Some(keys) = keys else {
                    self.leak(&all());
                    self.leak(&recv_srcs);
                    return all();
                };
                let mut result = Vec::new();
                for k in keys {
                    let Some(sum) = self.esc.sums.get(k) else {
                        continue;
                    };
                    if sum.leaks.first().copied().unwrap_or(false) {
                        self.leak(&recv_srcs);
                    }
                    if sum.to_ret.first().copied().unwrap_or(false) {
                        result.extend(recv_srcs.iter().cloned());
                    }
                    for (i, srcs) in arg_srcs.iter().enumerate() {
                        let idx = (i + 1).min(sum.leaks.len().saturating_sub(1));
                        if sum.leaks.get(idx).copied().unwrap_or(false) {
                            self.leak(srcs);
                        }
                        if sum.to_ret.get(idx).copied().unwrap_or(false) {
                            result.extend(srcs.iter().cloned());
                        }
                    }
                }
                let _ = spread;
                result
            }
            other => {
                // A call through a function value or a literal invoked in
                // place: its parameters are not summarized.
                let f = self.eval(other);
                self.leak(&all());
                let _ = f;
                all()
            }
        }
    }
}

/// Whether `ty` is a slice, array or map whose elements are numbers, strings or
/// booleans: reading one yields a value that is not itself a reference.
fn scalar_elems(ty: &str) -> bool {
    let elem = array_elem_ty(ty)
        .or_else(|| ty.strip_prefix("[]"))
        .or_else(|| {
            ty.split_once(']')
                .filter(|_| ty.starts_with("map["))
                .map(|(_, v)| v)
        });
    matches!(
        elem,
        Some(
            "int"
                | "int8"
                | "int16"
                | "int32"
                | "int64"
                | "uint"
                | "uint8"
                | "uint16"
                | "uint32"
                | "uint64"
                | "uintptr"
                | "float32"
                | "float64"
                | "complex64"
                | "complex128"
                | "bool"
                | "string"
                | "byte"
                | "rune"
        )
    )
}

/// Whether a bare name in call position is a type conversion rather than a
/// function: the predeclared types and the written composite spellings.
fn is_conversion_name(name: &str) -> bool {
    name.starts_with("[]")
        || name.starts_with("map[")
        || matches!(
            name,
            "int"
                | "int8"
                | "int16"
                | "int32"
                | "int64"
                | "uint"
                | "uint8"
                | "uint16"
                | "uint32"
                | "uint64"
                | "uintptr"
                | "float32"
                | "float64"
                | "string"
                | "bool"
                | "byte"
                | "rune"
                | "error"
                | "any"
        )
}

/// Every identifier mentioned in `body`, nested literals included.
fn collect_idents(body: &[Stmt], out: &mut HashSet<String>) {
    fn expr(e: &Expr, out: &mut HashSet<String>) {
        match e {
            Expr::Ident(n) => {
                out.insert(n.clone());
            }
            Expr::Unary { rhs, .. } => expr(rhs, out),
            Expr::Binary { lhs, rhs, .. } => {
                expr(lhs, out);
                expr(rhs, out);
            }
            Expr::Call { func, args, .. } => {
                expr(func, out);
                args.iter().for_each(|a| expr(a, out));
            }
            Expr::Selector { recv, .. } => expr(recv, out),
            Expr::Index { recv, index } => {
                expr(recv, out);
                expr(index, out);
            }
            Expr::Slice {
                recv,
                low,
                high,
                max,
            } => {
                expr(recv, out);
                for b in [low, high, max].into_iter().flatten() {
                    expr(b, out);
                }
            }
            Expr::SliceLit { elems, .. } => elems.iter().for_each(|x| expr(x, out)),
            Expr::MapLit { pairs, .. } => pairs.iter().for_each(|(k, v)| {
                expr(k, out);
                expr(v, out);
            }),
            Expr::StructLit { fields, .. } => fields.iter().for_each(|(_, v)| expr(v, out)),
            Expr::Make {
                len,
                cap,
                elem_zero,
                ..
            } => {
                for b in [len, cap].into_iter().flatten() {
                    expr(b, out);
                }
                expr(elem_zero, out);
            }
            Expr::MakeChan { cap: Some(c), .. } => expr(c, out),
            Expr::Recv { chan } => expr(chan, out),
            Expr::TypeAssert { expr: e, .. } => expr(e, out),
            Expr::FuncLit { body, .. } => collect_idents(body, out),
            _ => {}
        }
    }
    fn stmt(s: &Stmt, out: &mut HashSet<String>) {
        match s {
            Stmt::Var { init, .. } => {
                if let Some(e) = init {
                    expr(e, out);
                }
            }
            Stmt::Short { values, .. } => values.iter().for_each(|e| expr(e, out)),
            Stmt::Assign { target, value, .. } => {
                expr(target, out);
                expr(value, out);
            }
            Stmt::AssignMulti {
                targets, values, ..
            } => targets.iter().chain(values).for_each(|e| expr(e, out)),
            Stmt::IncDec { target, .. } => expr(target, out),
            Stmt::ExprStmt(e) => expr(e, out),
            Stmt::Return(vs, _) => vs.iter().for_each(|e| expr(e, out)),
            Stmt::If {
                init,
                cond,
                then,
                els,
                ..
            } => {
                if let Some(i) = init {
                    stmt(i, out);
                }
                expr(cond, out);
                collect_idents(then, out);
                collect_idents(els, out);
            }
            Stmt::For {
                init,
                cond,
                post,
                body,
                ..
            } => {
                for i in [init, post].into_iter().flatten() {
                    stmt(i, out);
                }
                if let Some(c) = cond {
                    expr(c, out);
                }
                collect_idents(body, out);
            }
            Stmt::ForRange { iter, body, .. } => {
                expr(iter, out);
                collect_idents(body, out);
            }
            Stmt::Go { call, .. } | Stmt::Defer { call, .. } => expr(call, out),
            Stmt::Send { chan, val, .. } => {
                expr(chan, out);
                expr(val, out);
            }
            Stmt::Select { cases, default, .. } => {
                for c in cases {
                    match &c.comm {
                        SelectComm::Recv { chan, .. } => expr(chan, out),
                        SelectComm::Send { chan, val } => {
                            expr(chan, out);
                            expr(val, out);
                        }
                    }
                    collect_idents(&c.body, out);
                }
                if let Some(d) = default {
                    collect_idents(d, out);
                }
            }
            Stmt::Switch {
                init,
                tag,
                cases,
                default,
                ..
            } => {
                if let Some(i) = init {
                    stmt(i, out);
                }
                if let Some(t) = tag {
                    expr(t, out);
                }
                for c in cases {
                    c.exprs.iter().for_each(|e| expr(e, out));
                    collect_idents(&c.body, out);
                }
                if let Some(d) = default {
                    collect_idents(d, out);
                }
            }
            Stmt::TypeSwitch {
                init,
                expr: e,
                cases,
                default,
                ..
            } => {
                if let Some(i) = init {
                    stmt(i, out);
                }
                expr(e, out);
                for c in cases {
                    collect_idents(&c.body, out);
                }
                if let Some(d) = default {
                    collect_idents(d, out);
                }
            }
            Stmt::Block(b) => collect_idents(b, out),
            Stmt::Fallthrough(_)
            | Stmt::Goto(..)
            | Stmt::Label(..)
            | Stmt::Break(..)
            | Stmt::Continue(..) => {}
        }
    }
    for s in body {
        stmt(s, out);
    }
}
