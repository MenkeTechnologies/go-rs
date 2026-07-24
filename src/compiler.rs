//! Lower the Go AST to a `fusevm::Chunk`.
//!
//! There is no bespoke VM or Go runtime here: statements and expressions emit
//! fusevm ops (`LoadInt`, `Add`, `GetVar`, `JumpIfFalse`, `Call`, …) through a
//! `ChunkBuilder`, and fusevm runs the chunk on its three-tier Cranelift JIT.
//!
//! `func main`'s body runs in the global scope (variables addressed by name via
//! `GetVar`/`SetVar`). Every other `func` is lowered to a subroutine, emitted
//! after `main` and jumped over; its locals live in call-frame slots
//! (`GetSlot`/`SetSlot`) so recursion never clobbers a caller's variables. Calls
//! resolve by name index through `Op::Call`.
//!
//! Go's `/` truncates for integer operands and divides as floating point
//! otherwise; the compiler tracks each value's static numeric type and appends
//! `Op::TruncInt` only for `int ÷ int`. String `+` (concatenation) and string
//! ordering are dispatched at runtime through the strict numeric hook installed
//! by [`crate::host`].

use crate::ast::*;
use crate::host;
use std::collections::{HashMap, HashSet};

use fusevm::{Chunk, ChunkBuilder, Op, Value};

/// The static numeric category of a value — drives `/` truncation and the
/// choice between numeric and string comparison ops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NumType {
    Int,
    Float,
    Str,
    Bool,
    Unknown,
}

/// Map a Go type name to its numeric category.
fn numtype_of_ty(ty: &str) -> NumType {
    match ty {
        "int" | "int8" | "int16" | "int32" | "int64" | "uint" | "uint8" | "uint16" | "uint32"
        | "uint64" | "byte" | "rune" | "uintptr" => NumType::Int,
        "float32" | "float64" => NumType::Float,
        "string" => NumType::Str,
        "bool" => NumType::Bool,
        _ => NumType::Unknown,
    }
}

/// A top-level function's signature, for call resolution and return typing.
struct FuncSig {
    arity: usize,
    result: NumType,
    /// The Go type name of the first result (for struct/method type inference).
    result_ty: String,
    /// Number of declared result values (for multi-value-return destructuring).
    nresults: usize,
    /// True if the last parameter is variadic (`args ...T`); the trailing call
    /// arguments are packed into a slice.
    variadic: bool,
}

/// A collected function literal, compiled to a `$lambda_N` subroutine.
struct LambdaInfo {
    params: Vec<Param>,
    body: Vec<Stmt>,
    /// Free variables captured from the enclosing scope, in capture order.
    captures: Vec<String>,
    /// Aligned with `captures`: whether each was captured by reference (a shared
    /// heap cell) versus by value. Reads/writes of a cell capture go through it.
    cell_captures: Vec<bool>,
}

/// A lexical scope inside a subroutine: local/parameter name → frame slot.
struct Scope {
    slots: HashMap<String, u16>,
    next_slot: u16,
}

impl Scope {
    fn new() -> Self {
        Scope {
            slots: HashMap::new(),
            next_slot: 0,
        }
    }

    /// Slot index for `name`, allocating a fresh one on first mention.
    fn slot(&mut self, name: &str) -> u16 {
        if let Some(&s) = self.slots.get(name) {
            return s;
        }
        let s = self.next_slot;
        self.next_slot += 1;
        self.slots.insert(name.to_string(), s);
        s
    }

    /// Whether `name` already has a slot (non-allocating).
    fn has(&self, name: &str) -> bool {
        self.slots.contains_key(name)
    }
}

/// Back-patch targets for one enclosing breakable construct (`for` loop or
/// `switch`). `break` targets the innermost; `continue` targets the innermost
/// non-switch (loop), so a `continue` inside a switch reaches the enclosing loop.
#[derive(Default)]
struct LoopScope {
    breaks: Vec<usize>,
    continues: Vec<usize>,
    is_switch: bool,
}

struct Compiler {
    b: ChunkBuilder,
    /// `None` while lowering `main` (global scope); `Some` inside a subroutine.
    scope: Option<Scope>,
    /// Static numeric category of the variables in the current function.
    types: HashMap<String, NumType>,
    /// Static Go type name of each variable in the current function (for struct
    /// value-copy and method dispatch).
    decl_types: HashMap<String, String>,
    /// Every top-level (non-method) function, by name (for call resolution).
    funcs: HashMap<String, FuncSig>,
    /// Package-level variable/constant names (declared at the top level of
    /// `main`, which runs in the global scope). A function references these as
    /// name-indexed globals (`GetVar`/`SetVar`), not local slots.
    globals: HashSet<String>,
    /// Struct type names declared with `type T struct`.
    structs: HashSet<String>,
    /// Each struct type's fields, in declaration order: `(name, type)`.
    struct_fields: HashMap<String, Vec<(String, String)>>,
    /// Method arities keyed by `(receiver type, method name)`.
    methods: HashMap<(String, String), usize>,
    /// Method result counts keyed by `(receiver type, method name)` — lets a
    /// `v, ok := recv.M()` destructure a multi-value method return.
    method_nresults: HashMap<(String, String), usize>,
    /// The stack of enclosing `for` loops (innermost last).
    loops: Vec<LoopScope>,
    /// `return`/jump-outs emitted inside `main`, patched to the end of `main`.
    main_exits: Vec<usize>,
    /// Monotonic counter for compiler-generated temporaries (`for … range`).
    temp_counter: u32,
    /// Function literals collected during lowering; each is compiled to a hidden
    /// `$lambda_N` subroutine after the named functions.
    lambdas: Vec<LambdaInfo>,
    /// Variables statically known to hold a specific closure (name → lambda id),
    /// so `f(args)` on such a variable dispatches directly.
    closure_vars: HashMap<String, i64>,
    /// While compiling a lambda body: its captured variables (name → index into
    /// the closure's captures). `emit_get` reads these from the closure (slot 0).
    active_captures: HashMap<String, u16>,
    /// When true, emit a per-statement `CallBuiltin(DBG_LINE)` marker so `--dap`
    /// can stop on statement lines. Normal runs leave this off (zero extra ops).
    debug: bool,
    /// True when the program contains an inline `rust {}` block (a
    /// `__rust_compile(...)` call), so a bare-name call may be an FFI export.
    has_ffi: bool,
    /// True while compiling a function/lambda/`main` whose body has `defer`
    /// statements — gates the defer-frame prologue and the return-time drain.
    fn_has_defer: bool,
    /// True when the program calls `panic`/`recover` anywhere — gates the panic
    /// unwind machinery (post-call checks + a per-function panic epilogue) so
    /// programs that never panic pay nothing.
    uses_panic: bool,
    /// Forward jumps (from `panic` sites and post-call unwind checks) to the
    /// current function's panic epilogue; patched when that epilogue is emitted.
    panic_jumps: Vec<usize>,
    /// This function's params/locals that are captured by a nested closure and
    /// so live in a shared heap cell (Go's capture-by-reference). Reads/writes go
    /// through the cell; a captured cell handle is shared with the closure.
    boxed: HashSet<String>,
    /// While compiling a lambda: which of its captures are cells (captured by
    /// reference). A cell capture is dereferenced on read and written through.
    active_cell_captures: HashSet<String>,
    /// The current function's named result variables (empty when results are
    /// unnamed). They are zero-initialized locals; `return e…` assigns them, a
    /// bare `return`/fall-off/recovered-panic returns their current values, and a
    /// deferred closure may mutate them (they are boxed when captured).
    named_results: Vec<String>,
}

/// Whether the program (a function body, recursing everywhere including nested
/// literals) calls `panic` or `recover` — the gate for the unwind machinery.
fn body_uses_panic(body: &[Stmt]) -> bool {
    fn ex(e: &Expr) -> bool {
        match e {
            Expr::Call { func, args, .. } => {
                matches!(func.as_ref(), Expr::Ident(n) if n == "panic" || n == "recover")
                    || ex(func)
                    || args.iter().any(ex)
            }
            Expr::Unary { rhs, .. } => ex(rhs),
            Expr::Binary { lhs, rhs, .. } => ex(lhs) || ex(rhs),
            Expr::Selector { recv, .. } => ex(recv),
            Expr::TypeAssert { expr, .. } => ex(expr),
            Expr::Index { recv, index } => ex(recv) || ex(index),
            Expr::FuncLit { body, .. } => body_uses_panic(body),
            Expr::SliceLit { elems, .. } => elems.iter().any(ex),
            Expr::MapLit { pairs, .. } => pairs.iter().any(|(k, v)| ex(k) || ex(v)),
            Expr::StructLit { fields, .. } => fields.iter().any(|(_, v)| ex(v)),
            Expr::Recv { chan } => ex(chan),
            _ => false,
        }
    }
    fn st(s: &Stmt) -> bool {
        match s {
            Stmt::Var { init, .. } => init.as_ref().is_some_and(ex),
            Stmt::Short { values, .. } => values.iter().any(ex),
            Stmt::Assign { target, value, .. } => ex(target) || ex(value),
            Stmt::AssignMulti {
                targets, values, ..
            } => targets.iter().any(ex) || values.iter().any(ex),
            Stmt::IncDec { target, .. } => ex(target),
            Stmt::ExprStmt(e) => ex(e),
            Stmt::Return(vs, _) => vs.iter().any(ex),
            Stmt::If {
                init,
                then,
                els,
                cond,
                ..
            } => {
                init.as_deref().is_some_and(st)
                    || ex(cond)
                    || body_uses_panic(then)
                    || body_uses_panic(els)
            }
            Stmt::For {
                init, cond, body, ..
            } => {
                init.as_deref().is_some_and(st)
                    || cond.as_ref().is_some_and(ex)
                    || body_uses_panic(body)
            }
            Stmt::ForRange { body, .. } => body_uses_panic(body),
            Stmt::Block(b) => body_uses_panic(b),
            Stmt::Go { call, .. } | Stmt::Defer { call, .. } => ex(call),
            Stmt::Send { chan, val, .. } => ex(chan) || ex(val),
            Stmt::Select { cases, default, .. } => {
                cases.iter().any(|c| body_uses_panic(&c.body))
                    || default.as_deref().is_some_and(body_uses_panic)
            }
            Stmt::Switch {
                init,
                tag,
                cases,
                default,
                ..
            } => {
                init.as_deref().is_some_and(st)
                    || tag.as_ref().is_some_and(ex)
                    || cases
                        .iter()
                        .any(|c| c.exprs.iter().any(ex) || body_uses_panic(&c.body))
                    || default.as_deref().is_some_and(body_uses_panic)
            }
            Stmt::TypeSwitch {
                init,
                expr,
                cases,
                default,
                ..
            } => {
                init.as_deref().is_some_and(st)
                    || ex(expr)
                    || cases.iter().any(|c| body_uses_panic(&c.body))
                    || default.as_deref().is_some_and(body_uses_panic)
            }
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::Fallthrough(_) => false,
        }
    }
    body.iter().any(st)
}

/// The set of a function's parameters/locals that are captured by some nested
/// closure and must therefore be *boxed* (stored in a shared heap cell) so a
/// closure's writes propagate — Go's capture-by-reference. Computed structurally
/// before the body is compiled: the intersection of (names free in a nested
/// function literal) and (this function's own params/locals).
fn boxed_vars(params: &[Param], body: &[Stmt]) -> HashSet<String> {
    let mut captured = HashSet::new();
    for s in body {
        collect_captured(s, &mut captured);
    }
    if captured.is_empty() {
        return HashSet::new();
    }
    let mut locals: HashSet<String> = params.iter().map(|p| p.name.clone()).collect();
    for s in body {
        collect_locals(s, &mut locals);
    }
    // Loop variables are excluded: Go 1.22 gives them per-iteration value
    // semantics, which capture-by-value already models correctly. Boxing them
    // into one shared cell would regress to pre-1.22 (all closures see the last
    // value).
    let mut loop_vars = HashSet::new();
    for s in body {
        collect_loop_vars(s, &mut loop_vars);
    }
    captured
        .intersection(&locals)
        .filter(|n| !loop_vars.contains(*n))
        .cloned()
        .collect()
}

/// Names introduced as loop variables (a `for … := …` init or `for … range`),
/// excluded from boxing (they keep Go 1.22 per-iteration value semantics).
fn collect_loop_vars(s: &Stmt, out: &mut HashSet<String>) {
    match s {
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                collect_locals(i, out);
            }
            body.iter().for_each(|s| collect_loop_vars(s, out));
        }
        Stmt::ForRange { key, val, body, .. } => {
            out.extend(key.iter().cloned());
            out.extend(val.iter().cloned());
            body.iter().for_each(|s| collect_loop_vars(s, out));
        }
        Stmt::If { then, els, .. } => {
            then.iter().for_each(|s| collect_loop_vars(s, out));
            els.iter().for_each(|s| collect_loop_vars(s, out));
        }
        Stmt::Block(b) => b.iter().for_each(|s| collect_loop_vars(s, out)),
        Stmt::Select { cases, default, .. } => {
            for c in cases {
                c.body.iter().for_each(|s| collect_loop_vars(s, out));
            }
            if let Some(d) = default {
                d.iter().for_each(|s| collect_loop_vars(s, out));
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                c.body.iter().for_each(|s| collect_loop_vars(s, out));
            }
            if let Some(d) = default {
                d.iter().for_each(|s| collect_loop_vars(s, out));
            }
        }
        _ => {}
    }
}

/// Add to `out` the free names of every function literal reachable in `s` (a
/// nested closure's free names bubble up through its enclosing literal).
fn collect_captured(s: &Stmt, out: &mut HashSet<String>) {
    fn ex(e: &Expr, out: &mut HashSet<String>) {
        match e {
            Expr::FuncLit { params, body, .. } => {
                let mut bound: HashSet<String> = params.iter().map(|p| p.name.clone()).collect();
                for s in body {
                    free_stmt(s, &mut bound, out);
                }
            }
            Expr::Unary { rhs, .. } => ex(rhs, out),
            Expr::Binary { lhs, rhs, .. } => {
                ex(lhs, out);
                ex(rhs, out);
            }
            Expr::Call { func, args, .. } => {
                ex(func, out);
                args.iter().for_each(|a| ex(a, out));
            }
            Expr::Selector { recv, .. } => ex(recv, out),
            Expr::TypeAssert { expr, .. } => ex(expr, out),
            Expr::Index { recv, index } => {
                ex(recv, out);
                ex(index, out);
            }
            Expr::SliceLit { elems, .. } => elems.iter().for_each(|e| ex(e, out)),
            Expr::MapLit { pairs, .. } => pairs.iter().for_each(|(k, v)| {
                ex(k, out);
                ex(v, out);
            }),
            Expr::StructLit { fields, .. } => fields.iter().for_each(|(_, v)| ex(v, out)),
            Expr::Make { len, elem_zero, .. } => {
                if let Some(l) = len {
                    ex(l, out);
                }
                ex(elem_zero, out);
            }
            Expr::MakeChan { cap: Some(c) } => ex(c, out),
            Expr::MakeChan { cap: None } => {}
            Expr::Recv { chan } => ex(chan, out),
            _ => {}
        }
    }
    walk_stmt_exprs(s, &mut |e| ex(e, out));
}

/// Add to `out` the names of variables declared at this function level (params
/// handled by the caller); does not descend into nested function literals.
fn collect_locals(s: &Stmt, out: &mut HashSet<String>) {
    match s {
        Stmt::Var { name, .. } => {
            out.insert(name.clone());
        }
        Stmt::Short { names, .. } => out.extend(names.iter().cloned()),
        Stmt::ForRange { key, val, body, .. } => {
            out.extend(key.iter().cloned());
            out.extend(val.iter().cloned());
            body.iter().for_each(|s| collect_locals(s, out));
        }
        Stmt::If {
            init, then, els, ..
        } => {
            if let Some(i) = init {
                collect_locals(i, out);
            }
            then.iter().for_each(|s| collect_locals(s, out));
            els.iter().for_each(|s| collect_locals(s, out));
        }
        Stmt::For {
            init, post, body, ..
        } => {
            if let Some(i) = init {
                collect_locals(i, out);
            }
            if let Some(p) = post {
                collect_locals(p, out);
            }
            body.iter().for_each(|s| collect_locals(s, out));
        }
        Stmt::Block(b) => b.iter().for_each(|s| collect_locals(s, out)),
        Stmt::Select { cases, default, .. } => {
            for c in cases {
                if let SelectComm::Recv { bind: Some(b), .. } = &c.comm {
                    out.insert(b.clone());
                }
                c.body.iter().for_each(|s| collect_locals(s, out));
            }
            if let Some(d) = default {
                d.iter().for_each(|s| collect_locals(s, out));
            }
        }
        Stmt::Switch {
            init,
            cases,
            default,
            ..
        } => {
            if let Some(i) = init {
                collect_locals(i, out);
            }
            for c in cases {
                c.body.iter().for_each(|s| collect_locals(s, out));
            }
            if let Some(d) = default {
                d.iter().for_each(|s| collect_locals(s, out));
            }
        }
        _ => {}
    }
}

/// Free-name walk of a statement: a referenced identifier not in `bound` is free
/// (added to `out`). `bound` grows monotonically (matching [`Compiler::fv_stmt`]).
fn free_stmt(s: &Stmt, bound: &mut HashSet<String>, out: &mut HashSet<String>) {
    let fe = free_expr;
    match s {
        Stmt::Var { name, init, .. } => {
            if let Some(e) = init {
                fe(e, bound, out);
            }
            bound.insert(name.clone());
        }
        Stmt::Short { names, values, .. } => {
            values.iter().for_each(|v| fe(v, bound, out));
            bound.extend(names.iter().cloned());
        }
        Stmt::Assign { target, value, .. } => {
            fe(target, bound, out);
            fe(value, bound, out);
        }
        Stmt::AssignMulti {
            targets, values, ..
        } => {
            targets.iter().for_each(|e| fe(e, bound, out));
            values.iter().for_each(|e| fe(e, bound, out));
        }
        Stmt::IncDec { target, .. } => fe(target, bound, out),
        Stmt::ExprStmt(e) => fe(e, bound, out),
        Stmt::Return(vs, _) => vs.iter().for_each(|e| fe(e, bound, out)),
        Stmt::If {
            init,
            cond,
            then,
            els,
            ..
        } => {
            if let Some(i) = init {
                free_stmt(i, bound, out);
            }
            fe(cond, bound, out);
            then.iter().for_each(|s| free_stmt(s, bound, out));
            els.iter().for_each(|s| free_stmt(s, bound, out));
        }
        Stmt::For {
            init,
            cond,
            post,
            body,
            ..
        } => {
            if let Some(i) = init {
                free_stmt(i, bound, out);
            }
            if let Some(c) = cond {
                fe(c, bound, out);
            }
            if let Some(p) = post {
                free_stmt(p, bound, out);
            }
            body.iter().for_each(|s| free_stmt(s, bound, out));
        }
        Stmt::ForRange {
            key,
            val,
            iter,
            body,
            ..
        } => {
            fe(iter, bound, out);
            bound.extend(key.iter().cloned());
            bound.extend(val.iter().cloned());
            body.iter().for_each(|s| free_stmt(s, bound, out));
        }
        Stmt::Go { call, .. } | Stmt::Defer { call, .. } => fe(call, bound, out),
        Stmt::Send { chan, val, .. } => {
            fe(chan, bound, out);
            fe(val, bound, out);
        }
        Stmt::Select { cases, default, .. } => {
            for c in cases {
                match &c.comm {
                    SelectComm::Recv { bind, chan } => {
                        fe(chan, bound, out);
                        if let Some(b) = bind {
                            bound.insert(b.clone());
                        }
                    }
                    SelectComm::Send { chan, val } => {
                        fe(chan, bound, out);
                        fe(val, bound, out);
                    }
                }
                c.body.iter().for_each(|s| free_stmt(s, bound, out));
            }
            if let Some(d) = default {
                d.iter().for_each(|s| free_stmt(s, bound, out));
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
                free_stmt(i, bound, out);
            }
            if let Some(t) = tag {
                fe(t, bound, out);
            }
            for c in cases {
                c.exprs.iter().for_each(|e| fe(e, bound, out));
                c.body.iter().for_each(|s| free_stmt(s, bound, out));
            }
            if let Some(d) = default {
                d.iter().for_each(|s| free_stmt(s, bound, out));
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
                free_stmt(i, bound, out);
            }
            fe(expr, bound, out);
            if let Some(b) = bind {
                bound.insert(b.clone());
            }
            for c in cases {
                c.body.iter().for_each(|s| free_stmt(s, bound, out));
            }
            if let Some(d) = default {
                d.iter().for_each(|s| free_stmt(s, bound, out));
            }
        }
        Stmt::Block(b) => b.iter().for_each(|s| free_stmt(s, bound, out)),
        Stmt::Break(_) | Stmt::Continue(_) | Stmt::Fallthrough(_) => {}
    }
}

/// Free-name walk of an expression (see [`free_stmt`]).
fn free_expr(e: &Expr, bound: &HashSet<String>, out: &mut HashSet<String>) {
    match e {
        Expr::Ident(n) => {
            if !bound.contains(n) {
                out.insert(n.clone());
            }
        }
        Expr::Unary { rhs, .. } => free_expr(rhs, bound, out),
        Expr::Binary { lhs, rhs, .. } => {
            free_expr(lhs, bound, out);
            free_expr(rhs, bound, out);
        }
        Expr::Call { func, args, .. } => {
            free_expr(func, bound, out);
            args.iter().for_each(|a| free_expr(a, bound, out));
        }
        Expr::Selector { recv, .. } => free_expr(recv, bound, out),
        Expr::TypeAssert { expr, .. } => free_expr(expr, bound, out),
        Expr::Index { recv, index } => {
            free_expr(recv, bound, out);
            free_expr(index, bound, out);
        }
        Expr::SliceLit { elems, .. } => elems.iter().for_each(|e| free_expr(e, bound, out)),
        Expr::MapLit { pairs, .. } => pairs.iter().for_each(|(k, v)| {
            free_expr(k, bound, out);
            free_expr(v, bound, out);
        }),
        Expr::StructLit { fields, .. } => fields.iter().for_each(|(_, v)| free_expr(v, bound, out)),
        Expr::Make { len, elem_zero, .. } => {
            if let Some(l) = len {
                free_expr(l, bound, out);
            }
            free_expr(elem_zero, bound, out);
        }
        Expr::MakeChan { cap: Some(c) } => free_expr(c, bound, out),
        Expr::MakeChan { cap: None } => {}
        Expr::Recv { chan } => free_expr(chan, bound, out),
        // A nested function literal: its free names (minus its own params) are
        // free in the enclosing one too.
        Expr::FuncLit { params, body, .. } => {
            let mut inner = bound.clone();
            inner.extend(params.iter().map(|p| p.name.clone()));
            for s in body {
                free_stmt(s, &mut inner, out);
            }
        }
        _ => {}
    }
}

/// Apply `f` to every expression directly in a statement (not descending into
/// nested statements' own expressions beyond the immediate ones); used to reach
/// function literals for capture analysis.
fn walk_stmt_exprs(s: &Stmt, f: &mut impl FnMut(&Expr)) {
    match s {
        Stmt::Var { init: Some(e), .. } => f(e),
        Stmt::Var { init: None, .. } => {}
        Stmt::Short { values, .. } => values.iter().for_each(&mut *f),
        Stmt::Assign { target, value, .. } => {
            f(target);
            f(value);
        }
        Stmt::AssignMulti {
            targets, values, ..
        } => {
            targets.iter().for_each(&mut *f);
            values.iter().for_each(&mut *f);
        }
        Stmt::IncDec { target, .. } => f(target),
        Stmt::ExprStmt(e) => f(e),
        Stmt::Return(vs, _) => vs.iter().for_each(&mut *f),
        Stmt::If {
            init,
            cond,
            then,
            els,
            ..
        } => {
            if let Some(i) = init {
                walk_stmt_exprs(i, f);
            }
            f(cond);
            then.iter().for_each(|s| walk_stmt_exprs(s, f));
            els.iter().for_each(|s| walk_stmt_exprs(s, f));
        }
        Stmt::For {
            init,
            cond,
            post,
            body,
            ..
        } => {
            if let Some(i) = init {
                walk_stmt_exprs(i, f);
            }
            if let Some(c) = cond {
                f(c);
            }
            if let Some(p) = post {
                walk_stmt_exprs(p, f);
            }
            body.iter().for_each(|s| walk_stmt_exprs(s, f));
        }
        Stmt::ForRange { iter, body, .. } => {
            f(iter);
            body.iter().for_each(|s| walk_stmt_exprs(s, f));
        }
        Stmt::Go { call, .. } | Stmt::Defer { call, .. } => f(call),
        Stmt::Send { chan, val, .. } => {
            f(chan);
            f(val);
        }
        Stmt::Select { cases, default, .. } => {
            for c in cases {
                match &c.comm {
                    SelectComm::Recv { chan, .. } => f(chan),
                    SelectComm::Send { chan, val } => {
                        f(chan);
                        f(val);
                    }
                }
                c.body.iter().for_each(|s| walk_stmt_exprs(s, f));
            }
            if let Some(d) = default {
                d.iter().for_each(|s| walk_stmt_exprs(s, f));
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
                walk_stmt_exprs(i, f);
            }
            if let Some(t) = tag {
                f(t);
            }
            for c in cases {
                c.exprs.iter().for_each(&mut *f);
                c.body.iter().for_each(|s| walk_stmt_exprs(s, f));
            }
            if let Some(d) = default {
                d.iter().for_each(|s| walk_stmt_exprs(s, f));
            }
        }
        Stmt::TypeSwitch {
            init,
            expr,
            cases,
            default,
            ..
        } => {
            if let Some(i) = init {
                walk_stmt_exprs(i, f);
            }
            f(expr);
            for c in cases {
                c.body.iter().for_each(|s| walk_stmt_exprs(s, f));
            }
            if let Some(d) = default {
                d.iter().for_each(|s| walk_stmt_exprs(s, f));
            }
        }
        Stmt::Block(b) => b.iter().for_each(|s| walk_stmt_exprs(s, f)),
        Stmt::Break(_) | Stmt::Continue(_) | Stmt::Fallthrough(_) => {}
    }
}

/// Whether `body` contains a `defer` at this function level (not descending into
/// nested function literals, whose defers belong to their own invocation).
fn body_has_defer(body: &[Stmt]) -> bool {
    body.iter().any(stmt_has_defer)
}

fn stmt_has_defer(s: &Stmt) -> bool {
    match s {
        Stmt::Defer { .. } => true,
        Stmt::If { then, els, .. } => body_has_defer(then) || body_has_defer(els),
        Stmt::For { body, .. } | Stmt::ForRange { body, .. } => body_has_defer(body),
        Stmt::Block(b) => body_has_defer(b),
        Stmt::Select { cases, default, .. } => {
            cases.iter().any(|c| body_has_defer(&c.body))
                || default.as_deref().is_some_and(body_has_defer)
        }
        Stmt::Switch { cases, default, .. } => {
            cases.iter().any(|c| body_has_defer(&c.body))
                || default.as_deref().is_some_and(body_has_defer)
        }
        _ => false,
    }
}

/// Lower a whole program to a runnable chunk.
pub fn compile(prog: &Program) -> Result<Chunk, String> {
    compile_with(prog, false)
}

/// Compile with per-statement `DBG_LINE` line markers for the `--dap` debugger.
/// Identical to [`compile`] except each statement is preceded by a marker
/// carrying its source line (see [`crate::host::DBG_LINE`]).
pub fn compile_debug(prog: &Program) -> Result<Chunk, String> {
    compile_with(prog, true)
}

/// Collect the package-level variable/constant names — those declared at the
/// top level of `main` (a `var`/`const`/`:=`, including grouped `const (…)` /
/// `var (…)` blocks, which lower to a `Block` of `Var`s). These are stored as
/// name-indexed globals, so functions reference them via `GetVar`/`SetVar`.
fn collect_globals(stmts: &[Stmt]) -> HashSet<String> {
    fn add(s: &Stmt, g: &mut HashSet<String>) {
        match s {
            Stmt::Var { name, .. } => {
                g.insert(name.clone());
            }
            Stmt::Short { names, .. } => {
                g.extend(names.iter().cloned());
            }
            // Grouped `const (…)` / `var (…)` blocks lower to a `Block` of `Var`s.
            Stmt::Block(b) => {
                for s in b {
                    add(s, g);
                }
            }
            _ => {}
        }
    }
    let mut g = HashSet::new();
    for s in stmts {
        add(s, &mut g);
    }
    g
}

fn compile_with(prog: &Program, debug: bool) -> Result<Chunk, String> {
    let structs: HashSet<String> = prog.types.iter().map(|t| t.name.clone()).collect();
    let struct_fields: HashMap<String, Vec<(String, String)>> = prog
        .types
        .iter()
        .map(|t| {
            (
                t.name.clone(),
                t.fields
                    .iter()
                    .map(|p| (p.name.clone(), p.ty.clone()))
                    .collect(),
            )
        })
        .collect();

    let mut funcs: HashMap<String, FuncSig> = HashMap::new();
    let mut methods: HashMap<(String, String), usize> = HashMap::new();
    let mut method_nresults: HashMap<(String, String), usize> = HashMap::new();
    for f in &prog.funcs {
        match &f.receiver {
            Some(r) => {
                methods.insert((base_type(&r.ty), f.name.clone()), f.params.len());
                method_nresults.insert((base_type(&r.ty), f.name.clone()), f.results.len());
            }
            None => {
                funcs.insert(
                    f.name.clone(),
                    FuncSig {
                        arity: f.params.len(),
                        result: f
                            .results
                            .first()
                            .map(|t| numtype_of_ty(t))
                            .unwrap_or(NumType::Unknown),
                        result_ty: f.results.first().cloned().unwrap_or_default(),
                        nresults: f.results.len(),
                        variadic: f.variadic,
                    },
                );
            }
        }
    }

    let has_ffi = body_has_ffi(&prog.main) || prog.funcs.iter().any(|f| body_has_ffi(&f.body));
    // Package-level names: variables/constants declared at the top level of
    // `main` (which, after linking, holds every package's init-order globals
    // ahead of `main`'s own body). Functions read these as globals.
    let globals = collect_globals(&prog.main);

    let mut c = Compiler {
        b: ChunkBuilder::new(),
        scope: None,
        types: HashMap::new(),
        decl_types: HashMap::new(),
        funcs,
        globals,
        structs,
        struct_fields,
        methods,
        method_nresults,
        loops: Vec::new(),
        main_exits: Vec::new(),
        temp_counter: 0,
        lambdas: Vec::new(),
        closure_vars: HashMap::new(),
        active_captures: HashMap::new(),
        debug,
        has_ffi,
        fn_has_defer: false,
        uses_panic: body_uses_panic(&prog.main)
            || prog.funcs.iter().any(|f| body_uses_panic(&f.body)),
        panic_jumps: Vec::new(),
        boxed: HashSet::new(),
        active_cell_captures: HashSet::new(),
        named_results: Vec::new(),
    };

    // ── main body (global scope) ──
    // A program that uses panic/recover routes runtime faults (divide-by-zero,
    // index-out-of-range, nil dereference) through the recoverable panic path.
    if c.uses_panic {
        c.b.emit(Op::CallBuiltin(host::GSET_PANIC_MODE, 0), 0);
        c.b.emit(Op::Pop, 0);
    }
    // main's globals captured by a closure are boxed (shared cells) too.
    c.boxed = boxed_vars(&[], &prog.main);
    c.fn_has_defer = body_has_defer(&prog.main);
    if c.fn_has_defer {
        c.b.emit(Op::CallBuiltin(host::GDEFER_ENTER, 0), 0);
        c.b.emit(Op::Pop, 0);
    }
    for s in &prog.main {
        c.stmt(s)?;
    }
    // `return` inside `main`, and any panic unwind, jump here; run any deferred
    // calls (a deferred `recover()` may clear the panic), then fall off.
    let end = c.b.current_pos();
    let exits = std::mem::take(&mut c.main_exits);
    let panics = std::mem::take(&mut c.panic_jumps);
    for op in exits.into_iter().chain(panics) {
        c.b.patch_jump(op, end);
    }
    if c.fn_has_defer {
        c.emit_defer_drain();
        c.b.emit(Op::CallBuiltin(host::GDEFER_LEAVE, 0), 0);
        c.b.emit(Op::Pop, 0);
        c.fn_has_defer = false;
    }
    // A panic that reached `main` unrecovered is fatal (prints + exits non-zero).
    if c.uses_panic {
        c.b.emit(Op::CallBuiltin(host::GPANIC_FINISH, 0), 0);
        c.b.emit(Op::Pop, 0);
    }

    // ── subroutine bodies, emitted after main and jumped over ──
    if !prog.funcs.is_empty() || !c.lambdas.is_empty() {
        let skip = c.b.emit(Op::Jump(0), 0);
        for f in &prog.funcs {
            c.compile_func(f)?;
        }
        // Compile every collected lambda; compiling one may append more (a
        // nested closure), so iterate by index until the list stops growing.
        let mut i = 0;
        while i < c.lambdas.len() {
            c.compile_lambda(i)?;
            i += 1;
        }
        let after = c.b.current_pos();
        c.b.patch_jump(skip, after);
    }

    Ok(c.b.build())
}

impl Compiler {
    fn compile_func(&mut self, f: &Func) -> Result<(), String> {
        let entry = self.b.current_pos();
        let name_idx = self.b.add_name(&sub_name(f));
        self.b.add_sub_entry(name_idx, entry);

        let mut scope = Scope::new();
        self.types.clear();
        self.decl_types.clear();

        // A method binds its receiver to slot 0; parameters follow.
        let mut slot = 0u16;
        if let Some(r) = &f.receiver {
            scope.slots.insert(r.name.clone(), slot);
            self.types.insert(r.name.clone(), numtype_of_ty(&r.ty));
            self.decl_types.insert(r.name.clone(), base_type(&r.ty));
            slot += 1;
        }
        for p in &f.params {
            scope.slots.insert(p.name.clone(), slot);
            self.types.insert(p.name.clone(), numtype_of_ty(&p.ty));
            self.decl_types.insert(p.name.clone(), base_type(&p.ty));
            slot += 1;
        }
        scope.next_slot = slot;
        self.scope = Some(scope);

        // Named results become zero-initialized locals the body may read/assign.
        self.named_results = if f.result_names.iter().any(|n| !n.is_empty()) {
            f.result_names.clone()
        } else {
            Vec::new()
        };

        // Params/locals captured by a nested closure are boxed (shared cells).
        // Named results participate too (a deferred closure may capture them), so
        // include them in the capture analysis.
        let mut real_params: Vec<Param> = Vec::new();
        if let Some(r) = &f.receiver {
            real_params.push(r.clone());
        }
        real_params.extend(f.params.iter().cloned());
        let mut analysis_params = real_params.clone();
        for (name, ty) in f.result_names.iter().zip(&f.results) {
            if !name.is_empty() {
                analysis_params.push(Param {
                    name: name.clone(),
                    ty: ty.clone(),
                });
            }
        }
        self.boxed = boxed_vars(&analysis_params, &f.body);

        // Prologue: pop args into their slots. The last argument is on top of
        // the stack, so bind slots high-to-low (receiver deepest, at slot 0).
        for i in (0..slot).rev() {
            self.b.emit(Op::SetSlot(i), f.line);
        }
        self.box_params(&real_params);

        // Bind the named results to their zero values (boxed when captured).
        for (name, ty) in f.result_names.iter().zip(&f.results) {
            if name.is_empty() {
                continue;
            }
            if self.structs.contains(&base_type(ty)) {
                self.struct_lit(&base_type(ty), &[])?;
            } else {
                self.emit_default(numtype_of_ty(ty), f.line);
            }
            self.types.insert(name.clone(), numtype_of_ty(ty));
            self.decl_types.insert(name.clone(), base_type(ty));
            self.emit_declare(name, f.line);
        }

        self.fn_has_defer = body_has_defer(&f.body);
        self.panic_jumps.clear();
        if self.fn_has_defer {
            self.b.emit(Op::CallBuiltin(host::GDEFER_ENTER, 0), f.line);
            self.b.emit(Op::Pop, f.line);
        }

        for s in &f.body {
            self.stmt(s)?;
        }
        // Fall-off: return the named results (their current values, possibly set
        // by a deferred func) or nil for unnamed results.
        if self.named_results.is_empty() {
            self.b.emit(Op::LoadUndef, f.line);
            self.emit_return(f.line);
        } else {
            self.emit_named_return(f.line);
        }
        self.emit_panic_epilogue(&f.results, f.line);

        self.fn_has_defer = false;
        self.boxed = HashSet::new();
        self.named_results = Vec::new();
        self.scope = None;
        Ok(())
    }

    /// Lower a function literal: emit its closure value (captured variables +
    /// lambda id) and register the lambda for later subroutine compilation.
    /// Returns the lambda id (for static closure-call dispatch).
    fn emit_funclit(&mut self, params: &[Param], body: &[Stmt]) -> i64 {
        let captures = self.free_vars(params, body);
        let id = self.lambdas.len() as i64;
        // Build the closure: push each captured value, then the target lambda's
        // subroutine name-index (so a dynamically-dispatched call can resolve it).
        // A by-reference (boxed) capture forwards the shared cell handle (raw),
        // so writes on either side propagate; a by-value capture forwards a copy.
        let cell_captures: Vec<bool> = captures.iter().map(|c| self.is_boxed(c)).collect();
        for c in &captures {
            if self.is_boxed(c) {
                self.emit_get_raw(c, 0);
            } else {
                self.emit_get(c, 0);
            }
        }
        let nidx = self.b.add_name(&format!("$lambda_{id}"));
        self.b.emit(Op::LoadInt(nidx as i64), 0);
        self.b.emit(
            Op::CallBuiltin(host::GCLOSURE_NEW, captures.len() as u8 + 1),
            0,
        );
        self.lambdas.push(LambdaInfo {
            params: params.to_vec(),
            body: body.to_vec(),
            captures,
            cell_captures,
        });
        id
    }

    /// Return from the current function: run any deferred calls (LIFO), drop the
    /// defer frame, then `ReturnValue`. The return value is already on the stack;
    /// the drain is stack-neutral above it.
    fn emit_return(&mut self, line: u32) {
        if self.fn_has_defer {
            self.emit_defer_drain();
            self.b.emit(Op::CallBuiltin(host::GDEFER_LEAVE, 0), line);
            self.b.emit(Op::Pop, line);
        }
        self.b.emit(Op::ReturnValue, line);
    }

    /// Return the current values of the named results. Deferred calls run *first*
    /// (a deferred `recover()` may assign a named result), so the values are read
    /// after the drain — this is why a named-result return can't reuse the
    /// value-on-stack-then-drain shape of [`Self::emit_return`].
    fn emit_named_return(&mut self, line: u32) {
        if self.fn_has_defer {
            self.emit_defer_drain();
            self.b.emit(Op::CallBuiltin(host::GDEFER_LEAVE, 0), line);
            self.b.emit(Op::Pop, line);
        }
        let names = self.named_results.clone();
        if names.len() >= 2 {
            for r in &names {
                self.emit_get(r, line);
            }
            self.b
                .emit(Op::CallBuiltin(host::GSLICE_LIT, names.len() as u8), line);
        } else if let Some(r) = names.first() {
            self.emit_get(r, line);
        } else {
            self.b.emit(Op::LoadUndef, line);
        }
        self.b.emit(Op::ReturnValue, line);
    }

    /// Emit a function's panic epilogue (the target of its `panic` sites and
    /// post-call unwind checks): drain this frame's defers — a deferred
    /// `recover()` may clear the panic — drop the frame, and return nil. If the
    /// panic is still live, the caller's post-call check propagates it. Emitted
    /// after the normal fall-off, so it is only reachable by an unwind jump.
    fn emit_panic_epilogue(&mut self, results: &[String], line: u32) {
        if self.panic_jumps.is_empty() {
            return;
        }
        let ep = self.b.current_pos();
        let jumps = std::mem::take(&mut self.panic_jumps);
        for j in jumps {
            self.b.patch_jump(j, ep);
        }
        // A named-result function returns its results' current values (a deferred
        // `recover()` may have assigned them). An unnamed-result function returns
        // the result types' zero values so a recovered call still has the right
        // shape.
        if !self.named_results.is_empty() {
            self.emit_named_return(line);
            return;
        }
        if results.len() >= 2 {
            for ty in results {
                self.emit_default(numtype_of_ty(ty), line);
            }
            self.b
                .emit(Op::CallBuiltin(host::GSLICE_LIT, results.len() as u8), line);
        } else if let Some(ty) = results.first() {
            self.emit_default(numtype_of_ty(ty), line);
        } else {
            self.b.emit(Op::LoadUndef, line);
        }
        self.emit_return(line);
    }

    /// After a user-function call, if a panic is now propagating, jump to the
    /// current function's panic epilogue (which drains defers and returns),
    /// carrying the unwind up the call chain. No-op unless the program panics.
    fn emit_panic_check(&mut self, line: u32) {
        if !self.uses_panic {
            return;
        }
        self.b.emit(Op::CallBuiltin(host::GPANIC_ACTIVE, 0), line);
        let j = self.b.emit(Op::JumpIfTrue(0), line);
        self.panic_jumps.push(j);
    }

    /// Emit the deferred-call drain loop: `while GDEFER_LEN() > 0 { c := pop; c() }`.
    /// Each deferred closure takes no arguments (its call was snapshotted at
    /// `defer` time), so it is invoked as `c(self=c)` via `Op::CallDynamic`.
    fn emit_defer_drain(&mut self) {
        let start = self.b.current_pos();
        self.b.emit(Op::CallBuiltin(host::GDEFER_LEN, 0), 0);
        let done = self.b.emit(Op::JumpIfFalse(0), 0);
        self.b.emit(Op::CallBuiltin(host::GDEFER_POP, 0), 0);
        self.emit_set("$dcpop", 0);
        self.emit_get("$dcpop", 0); // the closure, as its own "self"
        self.emit_get("$dcpop", 0);
        self.b.emit(Op::CallBuiltin(host::GCLOSURE_NAMEIDX, 1), 0);
        self.b.emit(Op::CallDynamic(1), 0);
        self.b.emit(Op::Pop, 0); // discard the deferred call's result
        self.b.emit(Op::Jump(start), 0);
        let end = self.b.current_pos();
        self.b.patch_jump(done, end);
    }

    /// Lower `defer <call>`: snapshot the callee value (a method receiver or a
    /// func-valued variable) and every argument into temporaries *now*, then push
    /// a zero-argument closure that re-invokes the call over those snapshots. The
    /// closure runs at function return via [`Self::emit_defer_drain`].
    fn compile_defer(&mut self, call: &Expr, line: u32) -> Result<(), String> {
        let Expr::Call { func, args, .. } = call else {
            return Err(format!(
                "go-rs: `defer` requires a function call (line {line})"
            ));
        };
        let n = self.temp_counter;
        self.temp_counter += 1;

        // Snapshot list: (temp name, expression, by_ref). `by_ref` snapshots keep
        // reference semantics (no struct copy) — a method receiver, so a deferred
        // pointer-receiver call sees mutations made after the `defer` (Go captures
        // the receiver pointer). Arguments are copied (Go evaluates them now).
        let mut temps: Vec<(String, Expr, bool)> = Vec::new();

        // Classify the callee. Package calls (`fmt.Println`), top-level funcs, and
        // builtins are referenced by name (they don't change); a method receiver
        // or a func-valued variable is snapshotted.
        let new_func: Expr = match func.as_ref() {
            Expr::Selector { recv, field } => {
                if matches!(recv.as_ref(), Expr::Ident(p) if is_package(p)) {
                    (**func).clone()
                } else {
                    let rt = format!("$dfr{n}");
                    temps.push((rt.clone(), (**recv).clone(), true));
                    Expr::Selector {
                        recv: Box::new(Expr::Ident(rt)),
                        field: field.clone(),
                    }
                }
            }
            Expr::Ident(name)
                if self.funcs.contains_key(name) || is_builtin_call(name) || name == "close" =>
            {
                (**func).clone()
            }
            Expr::Ident(name) => {
                let ft = format!("$dff{n}");
                temps.push((ft.clone(), Expr::Ident(name.clone()), true));
                Expr::Ident(ft)
            }
            other => other.clone(),
        };

        // Snapshot the arguments (by value).
        let mut new_args = Vec::new();
        for (i, a) in args.iter().enumerate() {
            let at = format!("$dfa{n}_{i}");
            temps.push((at.clone(), a.clone(), false));
            new_args.push(Expr::Ident(at));
        }

        // Evaluate each snapshot into its temporary local (recording types so the
        // deferred closure's body dispatches methods/func-values correctly).
        for (tname, texpr, by_ref) in &temps {
            let nt = self.infer(texpr);
            let dt = self.type_name(texpr);
            if *by_ref {
                self.expr(texpr)?;
            } else {
                self.emit_value(texpr)?;
            }
            self.types.insert(tname.clone(), nt);
            self.decl_types.insert(tname.clone(), dt);
            self.emit_set(tname, line);
        }

        // Build `func() { new_func(new_args) }`, capturing the snapshots by value,
        // and push it onto the current defer frame.
        let body = vec![Stmt::ExprStmt(Expr::Call {
            func: Box::new(new_func),
            args: new_args,
            spread: false,
            line,
        })];
        self.emit_funclit(&[], &body);
        self.b.emit(Op::CallBuiltin(host::GDEFER_PUSH, 1), line);
        self.b.emit(Op::Pop, line);
        Ok(())
    }

    /// Emit a call to a closure whose value is already on the stack (as the
    /// deepest argument, "self"): evaluate the args and call `$lambda_id`.
    fn emit_closure_call(&mut self, id: i64, args: &[Expr], line: u32) -> Result<(), String> {
        self.emit_closure_call_args(args)?;
        let idx = self.b.add_name(&format!("$lambda_{id}"));
        self.b.emit(Op::Call(idx, args.len() as u8 + 1), line);
        self.emit_panic_check(line);
        Ok(())
    }

    /// Evaluate the arguments of a closure call (struct-copied like any call).
    fn emit_closure_call_args(&mut self, args: &[Expr]) -> Result<(), String> {
        for a in args {
            self.emit_value(a)?;
        }
        Ok(())
    }

    /// Compile a collected lambda to a `$lambda_N` subroutine. Slot 0 is the
    /// closure itself (captured values read via `GCLOSURE_GET`); parameters take
    /// slots `1..`.
    fn compile_lambda(&mut self, id: usize) -> Result<(), String> {
        let params = self.lambdas[id].params.clone();
        let body = self.lambdas[id].body.clone();
        let captures = self.lambdas[id].captures.clone();
        let cell_captures = self.lambdas[id].cell_captures.clone();

        let entry = self.b.current_pos();
        let name_idx = self.b.add_name(&format!("$lambda_{id}"));
        self.b.add_sub_entry(name_idx, entry);

        let mut scope = Scope::new();
        self.types.clear();
        self.decl_types.clear();
        let mut slot = 1u16; // slot 0 reserved for the closure ("self")
        for p in &params {
            scope.slots.insert(p.name.clone(), slot);
            self.types.insert(p.name.clone(), numtype_of_ty(&p.ty));
            self.decl_types.insert(p.name.clone(), base_type(&p.ty));
            slot += 1;
        }
        scope.next_slot = slot;
        self.active_captures = captures
            .iter()
            .enumerate()
            .map(|(i, n)| (n.clone(), i as u16))
            .collect();
        // Captures that arrived as shared cells (captured by reference).
        self.active_cell_captures = captures
            .iter()
            .zip(&cell_captures)
            .filter(|(_, &cell)| cell)
            .map(|(n, _)| n.clone())
            .collect();
        // This lambda's own params/locals captured by a further-nested closure.
        let saved_boxed = std::mem::take(&mut self.boxed);
        self.boxed = boxed_vars(&params, &body);
        self.scope = Some(scope);

        // Prologue: bind the closure + params (closure deepest, at slot 0).
        for i in (0..slot).rev() {
            self.b.emit(Op::SetSlot(i), 0);
        }
        self.box_params(&params);

        self.fn_has_defer = body_has_defer(&body);
        let saved_panic_jumps = std::mem::take(&mut self.panic_jumps);
        if self.fn_has_defer {
            self.b.emit(Op::CallBuiltin(host::GDEFER_ENTER, 0), 0);
            self.b.emit(Op::Pop, 0);
        }

        for s in &body {
            self.stmt(s)?;
        }
        self.b.emit(Op::LoadUndef, 0);
        self.emit_return(0);
        self.emit_panic_epilogue(&[], 0);

        self.panic_jumps = saved_panic_jumps;
        self.boxed = saved_boxed;
        self.fn_has_defer = false;
        self.scope = None;
        self.active_captures.clear();
        self.active_cell_captures.clear();
        Ok(())
    }

    /// After the prologue, wrap each boxed parameter's value in a fresh cell so a
    /// nested closure that captures the parameter shares its storage.
    fn box_params(&mut self, params: &[Param]) {
        for p in params {
            if self.boxed.contains(&p.name) {
                self.emit_get_raw(&p.name, 0);
                self.b.emit(Op::CallBuiltin(host::GCELL_NEW, 1), 0);
                self.emit_set_raw(&p.name, 0);
            }
        }
    }

    /// The free variables of a lambda: names read in `body` that are neither its
    /// parameters/locals nor top-level functions, but are variables of the
    /// enclosing scope (captured by value).
    fn free_vars(&self, params: &[Param], body: &[Stmt]) -> Vec<String> {
        let mut bound: HashSet<String> = params.iter().map(|p| p.name.clone()).collect();
        let mut caps = Vec::new();
        for s in body {
            self.fv_stmt(s, &mut bound, &mut caps);
        }
        caps
    }

    /// True if `name` names a variable of the scope currently being compiled
    /// (a local/param/global, or a capture of an enclosing lambda).
    fn is_enclosing_var(&self, name: &str) -> bool {
        self.types.contains_key(name)
            || self.decl_types.contains_key(name)
            || self.active_captures.contains_key(name)
    }

    fn fv_stmt(&self, s: &Stmt, bound: &mut HashSet<String>, caps: &mut Vec<String>) {
        match s {
            Stmt::Var { name, init, .. } => {
                if let Some(e) = init {
                    self.fv_expr(e, bound, caps);
                }
                bound.insert(name.clone());
            }
            Stmt::Short { names, values, .. } => {
                for v in values {
                    self.fv_expr(v, bound, caps);
                }
                for n in names {
                    bound.insert(n.clone());
                }
            }
            Stmt::Assign { target, value, .. } => {
                self.fv_expr(target, bound, caps);
                self.fv_expr(value, bound, caps);
            }
            Stmt::AssignMulti {
                targets, values, ..
            } => {
                for e in targets.iter().chain(values) {
                    self.fv_expr(e, bound, caps);
                }
            }
            Stmt::IncDec { target, .. } => self.fv_expr(target, bound, caps),
            Stmt::ExprStmt(e) => self.fv_expr(e, bound, caps),
            Stmt::Return(vs, _) => {
                for e in vs {
                    self.fv_expr(e, bound, caps);
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
                    self.fv_stmt(i, bound, caps);
                }
                self.fv_expr(cond, bound, caps);
                for s in then.iter().chain(els) {
                    self.fv_stmt(s, bound, caps);
                }
            }
            Stmt::For {
                init,
                cond,
                post,
                body,
                ..
            } => {
                if let Some(i) = init {
                    self.fv_stmt(i, bound, caps);
                }
                if let Some(c) = cond {
                    self.fv_expr(c, bound, caps);
                }
                if let Some(p) = post {
                    self.fv_stmt(p, bound, caps);
                }
                for s in body {
                    self.fv_stmt(s, bound, caps);
                }
            }
            Stmt::ForRange {
                key,
                val,
                iter,
                body,
                ..
            } => {
                self.fv_expr(iter, bound, caps);
                if let Some(k) = key {
                    bound.insert(k.clone());
                }
                if let Some(v) = val {
                    bound.insert(v.clone());
                }
                for s in body {
                    self.fv_stmt(s, bound, caps);
                }
            }
            Stmt::Go { call, .. } => self.fv_expr(call, bound, caps),
            Stmt::Defer { call, .. } => self.fv_expr(call, bound, caps),
            Stmt::Send { chan, val, .. } => {
                self.fv_expr(chan, bound, caps);
                self.fv_expr(val, bound, caps);
            }
            Stmt::Select { cases, default, .. } => {
                for c in cases {
                    match &c.comm {
                        SelectComm::Recv { bind, chan } => {
                            self.fv_expr(chan, bound, caps);
                            if let Some(v) = bind {
                                bound.insert(v.clone());
                            }
                        }
                        SelectComm::Send { chan, val } => {
                            self.fv_expr(chan, bound, caps);
                            self.fv_expr(val, bound, caps);
                        }
                    }
                    for s in &c.body {
                        self.fv_stmt(s, bound, caps);
                    }
                }
                if let Some(d) = default {
                    for s in d {
                        self.fv_stmt(s, bound, caps);
                    }
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
                    self.fv_stmt(i, bound, caps);
                }
                if let Some(t) = tag {
                    self.fv_expr(t, bound, caps);
                }
                for c in cases {
                    for e in &c.exprs {
                        self.fv_expr(e, bound, caps);
                    }
                    for s in &c.body {
                        self.fv_stmt(s, bound, caps);
                    }
                }
                if let Some(d) = default {
                    for s in d {
                        self.fv_stmt(s, bound, caps);
                    }
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
                    self.fv_stmt(i, bound, caps);
                }
                self.fv_expr(expr, bound, caps);
                if let Some(b) = bind {
                    bound.insert(b.clone());
                }
                for c in cases {
                    for s in &c.body {
                        self.fv_stmt(s, bound, caps);
                    }
                }
                if let Some(d) = default {
                    for s in d {
                        self.fv_stmt(s, bound, caps);
                    }
                }
            }
            Stmt::Block(b) => {
                for s in b {
                    self.fv_stmt(s, bound, caps);
                }
            }
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::Fallthrough(_) => {}
        }
    }

    fn fv_expr(&self, e: &Expr, bound: &HashSet<String>, caps: &mut Vec<String>) {
        match e {
            Expr::Ident(n) => {
                if !bound.contains(n) && self.is_enclosing_var(n) && !caps.contains(n) {
                    caps.push(n.clone());
                }
            }
            Expr::Unary { rhs, .. } => self.fv_expr(rhs, bound, caps),
            Expr::Binary { lhs, rhs, .. } => {
                self.fv_expr(lhs, bound, caps);
                self.fv_expr(rhs, bound, caps);
            }
            Expr::Call { func, args, .. } => {
                self.fv_expr(func, bound, caps);
                for a in args {
                    self.fv_expr(a, bound, caps);
                }
            }
            Expr::Selector { recv, .. } => self.fv_expr(recv, bound, caps),
            Expr::TypeAssert { expr, .. } => self.fv_expr(expr, bound, caps),
            Expr::Index { recv, index } => {
                self.fv_expr(recv, bound, caps);
                self.fv_expr(index, bound, caps);
            }
            Expr::Slice { recv, low, high } => {
                self.fv_expr(recv, bound, caps);
                if let Some(e) = low {
                    self.fv_expr(e, bound, caps);
                }
                if let Some(e) = high {
                    self.fv_expr(e, bound, caps);
                }
            }
            Expr::SliceLit { elems, .. } => {
                for el in elems {
                    self.fv_expr(el, bound, caps);
                }
            }
            Expr::MapLit { pairs, .. } => {
                for (k, v) in pairs {
                    self.fv_expr(k, bound, caps);
                    self.fv_expr(v, bound, caps);
                }
            }
            Expr::StructLit { fields, .. } => {
                for (_, v) in fields {
                    self.fv_expr(v, bound, caps);
                }
            }
            Expr::Make { len, .. } => {
                if let Some(l) = len {
                    self.fv_expr(l, bound, caps);
                }
            }
            Expr::MakeChan { cap } => {
                if let Some(c) = cap {
                    self.fv_expr(c, bound, caps);
                }
            }
            Expr::Recv { chan } => self.fv_expr(chan, bound, caps),
            // A nested function literal: its own params are bound; its remaining
            // free vars that name our scope become our captures too (chaining).
            Expr::FuncLit { params, body, .. } => {
                let mut inner = bound.clone();
                for p in params {
                    inner.insert(p.name.clone());
                }
                for s in body {
                    self.fv_stmt(s, &mut inner, caps);
                }
            }
            Expr::Int(_) | Expr::Float(..) | Expr::Str(_) | Expr::Bool(_) => {}
        }
    }

    // ── variable access ────────────────────────────────────────────────────

    /// Whether `name` is captured by reference in the current function — a boxed
    /// local, or a cell capture inside a lambda (both live in a shared cell).
    fn is_boxed(&self, name: &str) -> bool {
        self.boxed.contains(name) || self.active_cell_captures.contains(name)
    }

    /// Push a variable's raw storage: the closure cell handle for a captured or
    /// boxed variable, otherwise its plain value. Callers deref (`GCELL_GET`)
    /// when they want the boxed value.
    fn emit_get_raw(&mut self, name: &str, line: u32) {
        // Inside a lambda, a captured variable is read from the closure (slot 0).
        if let Some(&idx) = self.active_captures.get(name) {
            self.b.emit(Op::GetSlot(0), line);
            self.b.emit(Op::LoadInt(idx as i64), line);
            self.b.emit(Op::CallBuiltin(host::GCLOSURE_GET, 2), line);
            return;
        }
        // Inside a function, a name that is not a local slot but is a package
        // global is read as a name-indexed global (`GetVar`), not an empty slot.
        if self.scope.is_some() && !self.scope_has(name) && self.globals.contains(name) {
            let idx = self.b.add_name(name);
            self.b.emit(Op::GetVar(idx), line);
            return;
        }
        match &mut self.scope {
            Some(scope) => {
                let slot = scope.slot(name);
                self.b.emit(Op::GetSlot(slot), line);
            }
            None => {
                let idx = self.b.add_name(name);
                self.b.emit(Op::GetVar(idx), line);
            }
        }
    }

    /// Whether `name` is already a slot in the current function's scope.
    fn scope_has(&self, name: &str) -> bool {
        self.scope.as_ref().is_some_and(|s| s.has(name))
    }

    /// Store the top of stack into a variable's raw storage (slot/global).
    fn emit_set_raw(&mut self, name: &str, line: u32) {
        // Assigning to a package global from inside a function writes the global
        // (`SetVar`), not a fresh local slot. A shadowing local declaration
        // pre-registers its slot in `emit_declare`, so `scope_has` is true there.
        if self.scope.is_some() && !self.scope_has(name) && self.globals.contains(name) {
            let idx = self.b.add_name(name);
            self.b.emit(Op::SetVar(idx), line);
            return;
        }
        match &mut self.scope {
            Some(scope) => {
                let slot = scope.slot(name);
                self.b.emit(Op::SetSlot(slot), line);
            }
            None => {
                let idx = self.b.add_name(name);
                self.b.emit(Op::SetVar(idx), line);
            }
        }
    }

    fn emit_get(&mut self, name: &str, line: u32) {
        self.emit_get_raw(name, line);
        if self.is_boxed(name) {
            // The raw value is the cell handle; dereference to the boxed value.
            self.b.emit(Op::CallBuiltin(host::GCELL_GET, 1), line);
        }
    }

    fn emit_set(&mut self, name: &str, line: u32) {
        if self.is_boxed(name) {
            // Store into the shared cell: stack is `[value]`, push the cell handle
            // above it, then `GCELL_SET` writes through (visible to every closure).
            self.emit_get_raw(name, line);
            self.b.emit(Op::CallBuiltin(host::GCELL_SET, 2), line);
        } else {
            self.emit_set_raw(name, line);
        }
    }

    /// Declare a variable, binding the value on the stack. A boxed variable is
    /// wrapped in a fresh cell so its closures share the storage.
    fn emit_declare(&mut self, name: &str, line: u32) {
        // Pre-register the local slot so a declaration that shadows a package
        // global binds a fresh local (rather than writing the global): after
        // this, `scope_has(name)` is true, so `emit_set_raw` uses the slot.
        if let Some(scope) = self.scope.as_mut() {
            scope.slot(name);
        }
        if self.boxed.contains(name) {
            self.b.emit(Op::CallBuiltin(host::GCELL_NEW, 1), line);
            self.emit_set_raw(name, line);
        } else {
            self.emit_set(name, line);
        }
    }

    // ── statements ─────────────────────────────────────────────────────────

    fn stmt(&mut self, s: &Stmt) -> Result<(), String> {
        // In debug mode, emit a line marker before the statement so `--dap` can
        // stop on it. `CallBuiltin` always pushes its return value, so pop it.
        let line = stmt_line(s);
        if self.debug && line != 0 {
            self.b.emit(Op::CallBuiltin(crate::host::DBG_LINE, 0), line);
            self.b.emit(Op::Pop, line);
        }
        match s {
            Stmt::Var {
                name,
                ty,
                init,
                line,
            } => {
                let nt = match (ty, init) {
                    (Some(t), _) => numtype_of_ty(t),
                    (None, Some(e)) => self.infer(e),
                    (None, None) => NumType::Unknown,
                };
                let decl_ty = match (ty, init) {
                    (Some(t), _) => base_type(t),
                    (None, Some(e)) => self.type_name(e),
                    (None, None) => String::new(),
                };
                // `var s T` where T is a *value* struct type → its zero value is a
                // struct with every field zeroed (so `s.f` and methods work). A
                // pointer `var p *T` is nil, not a zero struct.
                let is_pointer = ty.as_ref().is_some_and(|t| t.starts_with('*'));
                match init {
                    Some(e) => self.emit_rhs(name, e)?,
                    None if !is_pointer && self.structs.contains(&decl_ty) => {
                        self.struct_lit(&decl_ty, &[])?
                    }
                    None => self.emit_default(nt, *line),
                }
                self.types.insert(name.clone(), nt);
                self.decl_types.insert(name.clone(), decl_ty);
                self.emit_declare(name, *line);
            }
            Stmt::Short {
                names,
                values,
                line,
            } => {
                // `v, ok := x.(T)` — comma-ok type assertion: `ok` is whether the
                // runtime type matches; `v` is the value (unchecked).
                if names.len() == 2 && values.len() == 1 {
                    if let Expr::TypeAssert { expr, ty } = &values[0] {
                        let n = self.temp_counter;
                        self.temp_counter += 1;
                        let tmp = format!("$ta{n}");
                        self.expr(expr)?;
                        self.types.insert(tmp.clone(), NumType::Unknown);
                        self.emit_set(&tmp, *line);
                        // ok = typetag(tmp) == tag
                        self.emit_get(&tmp, *line);
                        self.b.emit(Op::CallBuiltin(host::GTYPETAG, 1), *line);
                        let c = self.b.add_constant(Value::str(type_to_tag(ty)));
                        self.b.emit(Op::LoadConst(c), *line);
                        self.b.emit(Op::StrEq, *line);
                        self.types.insert(names[1].clone(), NumType::Bool);
                        self.emit_declare(&names[1], *line);
                        // v = ok ? tmp : zero(T)  (Go zeroes v on a failed assert).
                        self.emit_get(&names[1], *line);
                        let to_zero = self.b.emit(Op::JumpIfFalse(0), *line);
                        self.emit_get(&tmp, *line);
                        let done = self.b.emit(Op::Jump(0), *line);
                        let zpos = self.b.current_pos();
                        self.b.patch_jump(to_zero, zpos);
                        self.emit_default(numtype_of_ty(ty), *line);
                        let end = self.b.current_pos();
                        self.b.patch_jump(done, end);
                        self.types.insert(names[0].clone(), numtype_of_ty(ty));
                        self.decl_types.insert(names[0].clone(), base_type(ty));
                        self.emit_declare(&names[0], *line);
                        return Ok(());
                    }
                    // `v, ok := m[k]` — comma-ok map lookup: `GMAP_GET2` yields a
                    // `[value, present]` pair, destructured into the two names.
                    if let Expr::Index { recv, index } = &values[0] {
                        let n = self.temp_counter;
                        self.temp_counter += 1;
                        let pair = format!("$mg{n}");
                        self.expr(recv)?;
                        self.expr(index)?;
                        self.b.emit(Op::CallBuiltin(host::GMAP_GET2, 2), *line);
                        self.types.insert(pair.clone(), NumType::Unknown);
                        self.emit_set(&pair, *line);
                        // v = pair[0]
                        self.emit_get(&pair, *line);
                        self.b.emit(Op::LoadInt(0), *line);
                        self.b.emit(Op::CallBuiltin(host::GINDEX_GET, 2), *line);
                        self.types.insert(names[0].clone(), NumType::Unknown);
                        self.decl_types.insert(names[0].clone(), String::new());
                        self.emit_declare(&names[0], *line);
                        // ok = pair[1]
                        self.emit_get(&pair, *line);
                        self.b.emit(Op::LoadInt(1), *line);
                        self.b.emit(Op::CallBuiltin(host::GINDEX_GET, 2), *line);
                        self.types.insert(names[1].clone(), NumType::Bool);
                        self.emit_declare(&names[1], *line);
                        return Ok(());
                    }
                }
                // `a, b := f()` where a user `func` returns exactly len(names)
                // values: destructure the returned tuple (a slice heap value).
                if names.len() >= 2
                    && values.len() == 1
                    && self.call_result_count(&values[0]) == Some(names.len())
                {
                    let n = self.temp_counter;
                    self.temp_counter += 1;
                    let tup = format!("$tup{n}");
                    self.expr(&values[0])?;
                    self.emit_set(&tup, *line);
                    for (i, name) in names.iter().enumerate() {
                        self.emit_get(&tup, *line);
                        self.b.emit(Op::LoadInt(i as i64), *line);
                        self.b.emit(Op::CallBuiltin(host::GINDEX_GET, 2), *line);
                        self.types.insert(name.clone(), NumType::Unknown);
                        self.decl_types.insert(name.clone(), String::new());
                        self.emit_declare(name, *line);
                    }
                }
                // `n, _ := strconv.Atoi(s)` — a single-value call (in go-rs) with
                // extra names: bind the first, pad the rest with nil (the common
                // comma-ok / (v, err) idiom over a builtin-backed call).
                else if names.len() > values.len()
                    && values.len() == 1
                    && matches!(&values[0], Expr::Call { .. })
                {
                    let e = &values[0];
                    let nt = self.infer(e);
                    let dt = self.type_name(e);
                    self.emit_rhs(&names[0], e)?;
                    self.types.insert(names[0].clone(), nt);
                    self.decl_types.insert(names[0].clone(), dt);
                    self.emit_declare(&names[0], *line);
                    for name in &names[1..] {
                        self.b.emit(Op::LoadUndef, *line);
                        self.types.insert(name.clone(), NumType::Unknown);
                        self.emit_declare(name, *line);
                    }
                } else if names.len() != values.len() {
                    return Err(format!(
                        "go-rs: assignment mismatch: {} variables but {} values (line {line})",
                        names.len(),
                        values.len()
                    ));
                } else {
                    for (name, e) in names.iter().zip(values) {
                        let nt = self.infer(e);
                        let dt = self.type_name(e);
                        self.emit_rhs(name, e)?;
                        self.types.insert(name.clone(), nt);
                        self.decl_types.insert(name.clone(), dt);
                        self.emit_declare(name, *line);
                    }
                }
            }
            Stmt::Assign {
                target,
                op,
                value,
                line,
            } => self.assign(target, *op, value, *line)?,
            Stmt::AssignMulti {
                targets,
                values,
                line,
            } => self.assign_multi(targets, values, *line)?,
            Stmt::IncDec { target, inc, line } => {
                let one = Expr::Int(1);
                let op = if *inc { AssignOp::Add } else { AssignOp::Sub };
                self.assign(target, op, &one, *line)?;
            }
            Stmt::ExprStmt(e) => {
                self.expr(e)?;
                // Every expression leaves exactly one value; a bare expression
                // statement discards it.
                self.b.emit(Op::Pop, 0);
            }
            Stmt::Return(vals, line) => match self.scope {
                // A named-result function: `return e…` assigns the named results
                // (Go allows explicit values even with named results), a bare
                // `return` keeps their current values; either way deferred calls
                // run, then the named results are returned.
                Some(_) if !self.named_results.is_empty() => {
                    if !vals.is_empty() {
                        let names = self.named_results.clone();
                        for (name, e) in names.iter().zip(vals) {
                            self.emit_value(e)?;
                            self.emit_set(name, *line);
                        }
                    }
                    self.emit_named_return(*line);
                }
                Some(_) => {
                    match vals.len() {
                        0 => {
                            self.b.emit(Op::LoadUndef, *line);
                        }
                        1 => self.emit_value(&vals[0])?,
                        // Multiple results are returned as one tuple (a slice
                        // heap value), destructured at the call site.
                        n => {
                            for e in vals {
                                self.expr(e)?;
                            }
                            self.b
                                .emit(Op::CallBuiltin(host::GSLICE_LIT, n as u8), *line);
                        }
                    }
                    self.emit_return(*line);
                }
                None => {
                    // `return` in `main` — evaluate for effect, then jump to end.
                    for e in vals {
                        self.expr(e)?;
                        self.b.emit(Op::Pop, *line);
                    }
                    let j = self.b.emit(Op::Jump(0), *line);
                    self.main_exits.push(j);
                }
            },
            Stmt::If {
                init,
                cond,
                then,
                els,
                ..
            } => {
                if let Some(init) = init {
                    self.stmt(init)?;
                }
                self.expr(cond)?;
                let jf = self.b.emit(Op::JumpIfFalse(0), 0);
                for s in then {
                    self.stmt(s)?;
                }
                if els.is_empty() {
                    let end = self.b.current_pos();
                    self.b.patch_jump(jf, end);
                } else {
                    let jmp = self.b.emit(Op::Jump(0), 0);
                    let else_start = self.b.current_pos();
                    self.b.patch_jump(jf, else_start);
                    for s in els {
                        self.stmt(s)?;
                    }
                    let end = self.b.current_pos();
                    self.b.patch_jump(jmp, end);
                }
            }
            Stmt::For {
                init,
                cond,
                post,
                body,
                ..
            } => self.compile_for(init, cond, post, body)?,
            Stmt::ForRange {
                key,
                val,
                iter,
                body,
                ..
            } => self.compile_for_range(key, val, iter, body)?,
            Stmt::Go { call, line } => {
                let Expr::Call { func, args, .. } = call else {
                    return Err(format!(
                        "go-rs: `go` requires a function call (line {line})"
                    ));
                };
                match func.as_ref() {
                    // `go f(args)` — a top-level function.
                    Expr::Ident(name) if self.funcs.contains_key(name) => {
                        for a in args {
                            self.emit_value(a)?;
                        }
                        let idx = self.b.add_name(name);
                        self.b.emit(Op::Go(idx, args.len() as u8), *line);
                    }
                    // `go f(args)` where `f` is a closure variable.
                    Expr::Ident(name) if self.closure_vars.contains_key(name) => {
                        let id = self.closure_vars[name];
                        self.emit_get(name, *line);
                        for a in args {
                            self.emit_value(a)?;
                        }
                        let idx = self.b.add_name(&format!("$lambda_{id}"));
                        self.b.emit(Op::Go(idx, args.len() as u8 + 1), *line);
                    }
                    // `go func(){ … }(args)` — an immediately-invoked closure.
                    Expr::FuncLit { params, body, .. } => {
                        let id = self.emit_funclit(params, body);
                        for a in args {
                            self.emit_value(a)?;
                        }
                        let idx = self.b.add_name(&format!("$lambda_{id}"));
                        self.b.emit(Op::Go(idx, args.len() as u8 + 1), *line);
                    }
                    _ => {
                        return Err(format!(
                        "go-rs: `go` requires a top-level function or closure call (line {line})"
                    ))
                    }
                }
            }
            // `fallthrough` is realized structurally by `compile_switch` (it
            // detects a case body ending in one); here it emits nothing.
            Stmt::Fallthrough(_) => {}
            Stmt::Defer { call, line } => self.compile_defer(call, *line)?,
            Stmt::Send { chan, val, line } => {
                self.expr(chan)?;
                self.expr(val)?;
                self.b.emit(Op::ChanSend, *line);
            }
            Stmt::Select {
                cases,
                default,
                line,
            } => self.compile_select(cases, default, *line)?,
            Stmt::Switch {
                init,
                tag,
                cases,
                default,
                line,
            } => self.compile_switch(init, tag, cases, default, *line)?,
            Stmt::TypeSwitch {
                init,
                bind,
                expr,
                cases,
                default,
                line,
            } => self.compile_type_switch(init, bind, expr, cases, default, *line)?,
            Stmt::Break(line) => {
                let j = self.b.emit(Op::Jump(0), *line);
                self.loops
                    .last_mut()
                    .ok_or_else(|| format!("go-rs: `break` outside a loop (line {line})"))?
                    .breaks
                    .push(j);
            }
            Stmt::Continue(line) => {
                let j = self.b.emit(Op::Jump(0), *line);
                // `continue` targets the innermost enclosing loop, skipping any
                // switch scopes in between.
                self.loops
                    .iter_mut()
                    .rev()
                    .find(|s| !s.is_switch)
                    .ok_or_else(|| format!("go-rs: `continue` outside a loop (line {line})"))?
                    .continues
                    .push(j);
            }
            Stmt::Block(stmts) => {
                for s in stmts {
                    self.stmt(s)?;
                }
            }
        }
        Ok(())
    }

    fn compile_for(
        &mut self,
        init: &Option<Box<Stmt>>,
        cond: &Option<Expr>,
        post: &Option<Box<Stmt>>,
        body: &[Stmt],
    ) -> Result<(), String> {
        if let Some(init) = init {
            self.stmt(init)?;
        }
        self.loops.push(LoopScope::default());
        let top = self.b.current_pos();
        // A condition, if present, exits the loop when false (patched to `end`
        // alongside every `break`).
        if let Some(c) = cond {
            self.expr(c)?;
            let jf = self.b.emit(Op::JumpIfFalse(0), 0);
            self.loops.last_mut().unwrap().breaks.push(jf);
        }
        for s in body {
            self.stmt(s)?;
        }
        // `continue` lands here — run the post statement, then re-test.
        let post_pos = self.b.current_pos();
        if let Some(p) = post {
            self.stmt(p)?;
        }
        self.b.emit(Op::Jump(top), 0);
        let end = self.b.current_pos();

        let scope = self.loops.pop().unwrap();
        for j in scope.continues {
            self.b.patch_jump(j, post_pos);
        }
        for j in scope.breaks {
            self.b.patch_jump(j, end);
        }
        Ok(())
    }

    /// Lower a `select`: push each case's channel descriptor `(ch, is_recv,
    /// send_val)`, run `Op::Select`, then a jump table over the chosen case index
    /// the scheduler pushed (with the received value for a `case v := <-ch`).
    /// Lower a `switch` to an if/else-if chain (no implicit fallthrough — the
    /// first matching case runs, then jumps to the end). With a tag, each case
    /// tests `tag == caseExpr` (any of a comma list); without one, each case
    /// expression is itself the boolean condition.
    fn compile_switch(
        &mut self,
        init: &Option<Box<Stmt>>,
        tag: &Option<Expr>,
        cases: &[SwitchCase],
        default: &Option<Vec<Stmt>>,
        line: u32,
    ) -> Result<(), String> {
        if let Some(init) = init {
            self.stmt(init)?;
        }

        // Evaluate the tag once into a temp (if present).
        let tag_tmp = if tag.is_some() {
            let n = self.temp_counter;
            self.temp_counter += 1;
            let t = format!("$sw{n}");
            Some(t)
        } else {
            None
        };
        if let (Some(t), Some(e)) = (&tag_tmp, tag) {
            let nt = self.infer(e);
            self.expr(e)?;
            let t = t.clone();
            self.types.insert(t.clone(), nt);
            self.emit_set(&t, line);
        }

        // A switch is breakable (but transparent to `continue`).
        self.loops.push(LoopScope {
            is_switch: true,
            ..Default::default()
        });

        let mut end_jumps = Vec::new();
        // A `fallthrough` in the previous case jumps to this case's body,
        // skipping its condition (patched when the body start is known).
        let mut pending_ft: Option<usize> = None;
        for case in cases {
            // Condition: OR of `tag == e` (tagged) or `e` (expression switch).
            let mut next_jumps = Vec::new();
            // Build: if none of the exprs match, jump to next case.
            for (k, e) in case.exprs.iter().enumerate() {
                match &tag_tmp {
                    Some(t) => {
                        let t = t.clone();
                        self.emit_get(&t, line);
                        self.expr(e)?;
                        self.emit_eq(t.as_str(), e, line);
                    }
                    None => self.expr(e)?,
                }
                // If this expr matches, fall through to the body; else try next.
                if k + 1 < case.exprs.len() {
                    // matched → jump into body; not matched → check next expr.
                    let to_body = self.b.emit(Op::JumpIfTrue(0), line);
                    next_jumps.push((true, to_body));
                } else {
                    let skip = self.b.emit(Op::JumpIfFalse(0), line);
                    next_jumps.push((false, skip));
                }
            }
            // Patch the "matched → body" jumps (and any pending fallthrough) here.
            let body_start = self.b.current_pos();
            if let Some(j) = pending_ft.take() {
                self.b.patch_jump(j, body_start);
            }
            for (is_true, j) in &next_jumps {
                if *is_true {
                    self.b.patch_jump(*j, body_start);
                }
            }
            let ends_ft = matches!(case.body.last(), Some(Stmt::Fallthrough(_)));
            for s in &case.body {
                self.stmt(s)?;
            }
            if ends_ft {
                // Transfer to the next case's body instead of ending the switch.
                pending_ft = Some(self.b.emit(Op::Jump(0), line));
            } else {
                end_jumps.push(self.b.emit(Op::Jump(0), line));
            }
            // The final "not matched → skip" jump lands on the next case.
            let next = self.b.current_pos();
            for (is_true, j) in next_jumps {
                if !is_true {
                    self.b.patch_jump(j, next);
                }
            }
        }
        // A `fallthrough` out of the last case falls into `default`.
        if let Some(j) = pending_ft.take() {
            let ds = self.b.current_pos();
            self.b.patch_jump(j, ds);
        }
        if let Some(body) = default {
            for s in body {
                self.stmt(s)?;
            }
        }

        let end = self.b.current_pos();
        for j in end_jumps {
            self.b.patch_jump(j, end);
        }
        let scope = self.loops.pop().unwrap();
        for j in scope.breaks {
            self.b.patch_jump(j, end);
        }
        Ok(())
    }

    /// Lower a type switch `switch [v :=] x.(type) { case T: … }` to a runtime
    /// type-tag dispatch: the value's tag is compared against each case type's
    /// tag; the first match binds `v` and runs its body.
    fn compile_type_switch(
        &mut self,
        init: &Option<Box<Stmt>>,
        bind: &Option<String>,
        expr: &Expr,
        cases: &[TypeSwitchCase],
        default: &Option<Vec<Stmt>>,
        line: u32,
    ) -> Result<(), String> {
        if let Some(init) = init {
            self.stmt(init)?;
        }
        let n = self.temp_counter;
        self.temp_counter += 1;
        let val = format!("$ts{n}");
        let tag = format!("$tstag{n}");
        // Stash the value and its runtime type tag.
        self.expr(expr)?;
        self.types.insert(val.clone(), NumType::Unknown);
        self.emit_set(&val, line);
        self.emit_get(&val, line);
        self.b.emit(Op::CallBuiltin(host::GTYPETAG, 1), line);
        self.types.insert(tag.clone(), NumType::Str);
        self.emit_set(&tag, line);

        self.loops.push(LoopScope {
            is_switch: true,
            ..Default::default()
        });
        let mut end_jumps = Vec::new();
        for case in cases {
            let mut match_jumps = Vec::new();
            let mut skip_jumps = Vec::new();
            for (k, ty) in case.types.iter().enumerate() {
                let ctag = type_to_tag(ty);
                if ctag.is_empty() {
                    // An interface type (`any`) matches unconditionally.
                    match_jumps.push(self.b.emit(Op::Jump(0), line));
                    break;
                }
                self.emit_get(&tag, line);
                let c = self.b.add_constant(Value::str(ctag));
                self.b.emit(Op::LoadConst(c), line);
                self.b.emit(Op::StrEq, line);
                if k + 1 < case.types.len() {
                    match_jumps.push(self.b.emit(Op::JumpIfTrue(0), line));
                } else {
                    skip_jumps.push(self.b.emit(Op::JumpIfFalse(0), line));
                }
            }
            let body_start = self.b.current_pos();
            for j in &match_jumps {
                self.b.patch_jump(*j, body_start);
            }
            // Bind the value to `v` inside the body.
            if let Some(name) = bind {
                self.emit_get(&val, line);
                self.types.insert(name.clone(), NumType::Unknown);
                self.decl_types.insert(name.clone(), String::new());
                self.emit_declare(name, line);
            }
            for s in &case.body {
                self.stmt(s)?;
            }
            end_jumps.push(self.b.emit(Op::Jump(0), line));
            let next = self.b.current_pos();
            for j in skip_jumps {
                self.b.patch_jump(j, next);
            }
        }
        if let Some(body) = default {
            if let Some(name) = bind {
                self.emit_get(&val, line);
                self.types.insert(name.clone(), NumType::Unknown);
                self.emit_declare(name, line);
            }
            for s in body {
                self.stmt(s)?;
            }
        }
        let end = self.b.current_pos();
        for j in end_jumps {
            self.b.patch_jump(j, end);
        }
        let scope = self.loops.pop().unwrap();
        for j in scope.breaks {
            self.b.patch_jump(j, end);
        }
        Ok(())
    }

    /// Emit an equality compare between a tag temp and a case expression, picking
    /// string vs numeric comparison from the operand types.
    fn emit_eq(&mut self, tag_tmp: &str, case_expr: &Expr, line: u32) {
        // Both operands are already on the stack (tag, then case expr).
        let op = if self.infer(&Expr::Ident(tag_tmp.to_string())) == NumType::Str
            || self.infer(case_expr) == NumType::Str
        {
            Op::StrEq
        } else {
            Op::NumEq
        };
        self.b.emit(op, line);
    }

    fn compile_select(
        &mut self,
        cases: &[SelectClause],
        default: &Option<Vec<Stmt>>,
        line: u32,
    ) -> Result<(), String> {
        let n = self.temp_counter;
        self.temp_counter += 1;
        let si = format!("$si{n}");
        let sv = format!("$sv{n}");

        for c in cases {
            match &c.comm {
                SelectComm::Recv { chan, .. } => {
                    self.expr(chan)?;
                    self.b.emit(Op::LoadInt(1), line); // is_recv = 1
                    self.b.emit(Op::LoadInt(0), line); // (no send value)
                }
                SelectComm::Send { chan, val } => {
                    self.expr(chan)?;
                    self.b.emit(Op::LoadInt(0), line); // is_recv = 0
                    self.expr(val)?;
                }
            }
        }
        let has_default = if default.is_some() { 1 } else { 0 };
        self.b
            .emit(Op::Select(cases.len() as u8, has_default), line);
        // Stack: [recv_value, case_index] — index on top.
        self.emit_set(&si, line);
        self.emit_set(&sv, line);
        self.types.insert(si.clone(), NumType::Int);

        let mut end_jumps = Vec::new();
        for (i, c) in cases.iter().enumerate() {
            self.emit_get(&si, line);
            self.b.emit(Op::LoadInt(i as i64), line);
            self.b.emit(Op::NumEq, line);
            let jf = self.b.emit(Op::JumpIfFalse(0), line);
            if let SelectComm::Recv { bind: Some(v), .. } = &c.comm {
                self.emit_get(&sv, line);
                self.emit_set(v, line);
                self.types.insert(v.clone(), NumType::Unknown);
            }
            for s in &c.body {
                self.stmt(s)?;
            }
            end_jumps.push(self.b.emit(Op::Jump(0), line));
            let next = self.b.current_pos();
            self.b.patch_jump(jf, next);
        }
        // The `default` case runs when no real case index matched.
        if let Some(dbody) = default {
            for s in dbody {
                self.stmt(s)?;
            }
        }
        let end = self.b.current_pos();
        for j in end_jumps {
            self.b.patch_jump(j, end);
        }
        Ok(())
    }

    /// Lower a parallel assignment `t… = v…`. Right-hand sides are evaluated into
    /// temporaries *first* (so `a, b = b, a` swaps), then each temp is assigned to
    /// its target. Also handles `a, b = f()` where a call returns exactly as many
    /// values as there are targets (destructuring the returned tuple).
    fn assign_multi(&mut self, targets: &[Expr], values: &[Expr], line: u32) -> Result<(), String> {
        let n = self.temp_counter;
        self.temp_counter += 1;

        // `a, b = f()` — one call yielding len(targets) values.
        if targets.len() >= 2
            && values.len() == 1
            && self.call_result_count(&values[0]) == Some(targets.len())
        {
            let tup = format!("$am{n}");
            self.expr(&values[0])?;
            self.emit_set(&tup, line);
            for (i, target) in targets.iter().enumerate() {
                self.emit_get(&tup, line);
                self.b.emit(Op::LoadInt(i as i64), line);
                self.b.emit(Op::CallBuiltin(host::GINDEX_GET, 2), line);
                let tmp = format!("$amv{n}_{i}");
                self.types.insert(tmp.clone(), NumType::Unknown);
                self.emit_set(&tmp, line);
                self.assign(target, AssignOp::Set, &Expr::Ident(tmp), line)?;
            }
            return Ok(());
        }

        // `v, _ = strconv.Atoi(s)` — a single-value (builtin-backed) call with
        // extra targets: assign the first, pad the rest with nil (the `(v, err)`
        // idiom over a call go-rs models as single-valued).
        if targets.len() > values.len()
            && values.len() == 1
            && matches!(&values[0], Expr::Call { .. })
        {
            let tmp = format!("$am{n}");
            self.types.insert(tmp.clone(), self.infer(&values[0]));
            self.decl_types
                .insert(tmp.clone(), self.type_name(&values[0]));
            self.emit_value(&values[0])?;
            self.emit_set(&tmp, line);
            self.assign(&targets[0], AssignOp::Set, &Expr::Ident(tmp), line)?;
            let niltmp = format!("$amnil{n}");
            self.b.emit(Op::LoadUndef, line);
            self.types.insert(niltmp.clone(), NumType::Unknown);
            self.emit_set(&niltmp, line);
            for target in &targets[1..] {
                self.assign(target, AssignOp::Set, &Expr::Ident(niltmp.clone()), line)?;
            }
            return Ok(());
        }

        if targets.len() != values.len() {
            return Err(format!(
                "go-rs: assignment mismatch: {} targets but {} values (line {line})",
                targets.len(),
                values.len()
            ));
        }
        // Evaluate every value into a temp, then assign to targets.
        let mut tmps = Vec::new();
        for (i, v) in values.iter().enumerate() {
            let tmp = format!("$amv{n}_{i}");
            self.types.insert(tmp.clone(), self.infer(v));
            self.decl_types.insert(tmp.clone(), self.type_name(v));
            self.emit_value(v)?;
            self.emit_set(&tmp, line);
            tmps.push(tmp);
        }
        for (target, tmp) in targets.iter().zip(tmps) {
            self.assign(target, AssignOp::Set, &Expr::Ident(tmp), line)?;
        }
        Ok(())
    }

    /// Lower an assignment `target op= value` where `target` is an lvalue: a
    /// bare identifier, an index (`x[i]`), or a struct field (`x.f`).
    fn assign(
        &mut self,
        target: &Expr,
        op: AssignOp,
        value: &Expr,
        line: u32,
    ) -> Result<(), String> {
        match target {
            Expr::Ident(name) => {
                if op == AssignOp::Set {
                    self.emit_rhs(name, value)?;
                } else {
                    self.emit_get(name, line);
                    self.expr(value)?;
                    let l = self.types.get(name).copied().unwrap_or(NumType::Unknown);
                    let r = self.infer(value);
                    self.emit_arith(assign_binop(op), l, r, line);
                }
                self.emit_set(name, line);
            }
            Expr::Index { recv, index } => {
                self.expr(recv)?;
                self.expr(index)?;
                if op == AssignOp::Set {
                    self.expr(value)?;
                } else {
                    self.b.emit(Op::Dup2, line);
                    self.b.emit(Op::CallBuiltin(host::GINDEX_GET, 2), line);
                    self.expr(value)?;
                    self.emit_arith(assign_binop(op), NumType::Unknown, self.infer(value), line);
                }
                self.b.emit(Op::CallBuiltin(host::GINDEX_SET, 3), line);
                self.b.emit(Op::Pop, line);
                self.emit_panic_check(line); // index out of range is recoverable
            }
            Expr::Selector { recv, field } => {
                self.expr(recv)?;
                let c = self.b.add_constant(Value::str(field.clone()));
                self.b.emit(Op::LoadConst(c), line);
                if op == AssignOp::Set {
                    self.expr(value)?;
                } else {
                    self.b.emit(Op::Dup2, line);
                    self.b.emit(Op::CallBuiltin(host::GFIELD_GET, 2), line);
                    self.expr(value)?;
                    self.emit_arith(assign_binop(op), NumType::Unknown, self.infer(value), line);
                }
                self.b.emit(Op::CallBuiltin(host::GFIELD_SET, 3), line);
                self.b.emit(Op::Pop, line);
                self.emit_panic_check(line); // nil dereference is recoverable
            }
            _ => {
                return Err(format!(
                    "go-rs: cannot assign to this expression (line {line})"
                ))
            }
        }
        Ok(())
    }

    /// Lower `for [k[, v]] := range iter { body }` over a slice, map, or string.
    /// Iterates a host-computed key slice (`GRANGE_KEYS`) uniformly: `k` binds
    /// each key (index for a slice/string, key for a map); `v` binds `iter[k]`.
    fn compile_for_range(
        &mut self,
        key: &Option<String>,
        val: &Option<String>,
        iter: &Expr,
        body: &[Stmt],
    ) -> Result<(), String> {
        let n = self.temp_counter;
        self.temp_counter += 1;
        let it = format!("$it{n}");
        let keys = format!("$keys{n}");
        let i = format!("$i{n}");

        // $it = iter; $keys = GRANGE_KEYS($it); $i = 0
        self.expr(iter)?;
        self.emit_set(&it, 0);
        self.emit_get(&it, 0);
        self.b.emit(Op::CallBuiltin(host::GRANGE_KEYS, 1), 0);
        self.emit_set(&keys, 0);
        self.b.emit(Op::LoadInt(0), 0);
        self.emit_set(&i, 0);

        self.loops.push(LoopScope::default());
        let top = self.b.current_pos();
        // if $i >= len($keys) break
        self.emit_get(&i, 0);
        self.emit_get(&keys, 0);
        self.b.emit(Op::CallBuiltin(host::GLEN, 1), 0);
        self.b.emit(Op::NumLt, 0);
        let jf = self.b.emit(Op::JumpIfFalse(0), 0);
        self.loops.last_mut().unwrap().breaks.push(jf);

        // key := $keys[$i]
        if let Some(k) = key {
            self.emit_get(&keys, 0);
            self.emit_get(&i, 0);
            self.b.emit(Op::CallBuiltin(host::GINDEX_GET, 2), 0);
            self.emit_set(k, 0);
            self.types.insert(k.clone(), NumType::Unknown);
        }
        // val := GRANGE_VAL($it, key)  — the loop value for the current key. This
        // indexes a slice/map element but decodes the rune of a string (Go ranges
        // strings by rune, so the value is a code point, not a byte).
        if let Some(v) = val {
            self.emit_get(&it, 0);
            self.emit_get(&keys, 0);
            self.emit_get(&i, 0);
            self.b.emit(Op::CallBuiltin(host::GINDEX_GET, 2), 0);
            self.b.emit(Op::CallBuiltin(host::GRANGE_VAL, 2), 0);
            self.emit_set(v, 0);
            self.types.insert(v.clone(), NumType::Unknown);
        }

        for s in body {
            self.stmt(s)?;
        }

        // continue lands here: $i++ then re-test
        let post_pos = self.b.current_pos();
        self.emit_get(&i, 0);
        self.b.emit(Op::LoadInt(1), 0);
        self.b.emit(Op::Add, 0);
        self.emit_set(&i, 0);
        self.b.emit(Op::Jump(top), 0);
        let end = self.b.current_pos();

        let scope = self.loops.pop().unwrap();
        for j in scope.continues {
            self.b.patch_jump(j, post_pos);
        }
        for j in scope.breaks {
            self.b.patch_jump(j, end);
        }
        Ok(())
    }

    /// Emit the default zero value for a declared-without-initializer variable.
    fn emit_default(&mut self, nt: NumType, line: u32) {
        match nt {
            NumType::Int => self.b.emit(Op::LoadInt(0), line),
            NumType::Float => self.b.emit(Op::LoadFloat(0.0), line),
            NumType::Bool => self.b.emit(Op::LoadFalse, line),
            NumType::Str => {
                let c = self.b.add_constant(Value::str(""));
                self.b.emit(Op::LoadConst(c), line)
            }
            NumType::Unknown => self.b.emit(Op::LoadUndef, line),
        };
    }

    // ── expressions ────────────────────────────────────────────────────────

    fn expr(&mut self, e: &Expr) -> Result<(), String> {
        match e {
            Expr::Int(n) => {
                self.b.emit(Op::LoadInt(*n), 0);
            }
            Expr::Float(f, _) => {
                self.b.emit(Op::LoadFloat(*f), 0);
            }
            Expr::Str(s) => {
                let c = self.b.add_constant(Value::str(s.clone()));
                self.b.emit(Op::LoadConst(c), 0);
            }
            Expr::Bool(b) => {
                self.b
                    .emit(if *b { Op::LoadTrue } else { Op::LoadFalse }, 0);
            }
            Expr::Ident(name) => self.emit_get(name, 0),
            Expr::Unary { op, rhs } => {
                // `&x` / `*p` are reference/identity on go-rs's heap handles — no
                // copy, no op. Emitting the operand yields the shared handle, so a
                // pointer sees the same struct the original variable holds.
                if matches!(op, UnOp::Addr | UnOp::Deref) {
                    self.expr(rhs)?;
                } else {
                    self.expr(rhs)?;
                    self.b.emit(
                        match op {
                            UnOp::Neg => Op::Negate,
                            UnOp::Not => Op::LogNot,
                            UnOp::BitNot => Op::BitNot,
                            UnOp::Addr | UnOp::Deref => unreachable!(),
                        },
                        0,
                    );
                }
            }
            Expr::Binary { op, lhs, rhs } => {
                // Constant float expression: evaluate exactly (rational) and round
                // to f64 once, matching Go's arbitrary-precision constant rules.
                if self.infer(e) == NumType::Float {
                    if let Some(f) = fold_const_float(e) {
                        self.b.emit(Op::LoadFloat(f), 0);
                        return Ok(());
                    }
                }
                self.binary(*op, lhs, rhs)?
            }
            Expr::Call {
                func,
                args,
                spread,
                line,
            } => self.call(func, args, *spread, *line)?,
            // A single-result type assertion `x.(T)` — check and panic on
            // mismatch. (The comma-ok form is handled in the `Short` statement.)
            Expr::TypeAssert { expr, ty } => {
                self.expr(expr)?;
                let c = self.b.add_constant(Value::str(type_to_tag(ty)));
                self.b.emit(Op::LoadConst(c), 0);
                self.b.emit(Op::CallBuiltin(host::GASSERT, 2), 0);
                self.emit_panic_check(0);
            }
            // A bare selector `x.f` is a package constant (`math.Pi`) or a
            // struct field read.
            Expr::Selector { recv, field } => {
                if let Expr::Ident(pkg) = recv.as_ref() {
                    if let Some(v) = host::stdlib::resolve_const(pkg, field) {
                        let c = self.b.add_constant(v);
                        self.b.emit(Op::LoadConst(c), 0);
                        return Ok(());
                    }
                }
                self.expr(recv)?;
                let c = self.b.add_constant(Value::str(field.clone()));
                self.b.emit(Op::LoadConst(c), 0);
                self.b.emit(Op::CallBuiltin(host::GFIELD_GET, 2), 0);
                self.emit_panic_check(0); // nil dereference is recoverable
            }
            Expr::Index { recv, index } => {
                self.expr(recv)?;
                self.expr(index)?;
                self.b.emit(Op::CallBuiltin(host::GINDEX_GET, 2), 0);
                self.emit_panic_check(0); // index out of range is recoverable
            }
            Expr::Slice { recv, low, high } => {
                // `recv[low:high]`: push recv, low (or -1), high (or -1).
                self.expr(recv)?;
                match low {
                    Some(e) => self.expr(e)?,
                    None => {
                        self.b.emit(Op::LoadInt(-1), 0);
                    }
                }
                match high {
                    Some(e) => self.expr(e)?,
                    None => {
                        self.b.emit(Op::LoadInt(-1), 0);
                    }
                }
                self.b.emit(Op::CallBuiltin(host::GSLICE_SUB, 3), 0);
            }
            Expr::SliceLit { elems, .. } => {
                for e in elems {
                    self.expr(e)?;
                }
                self.b
                    .emit(Op::CallBuiltin(host::GSLICE_LIT, elems.len() as u8), 0);
            }
            Expr::MapLit { pairs, .. } => {
                for (k, v) in pairs {
                    self.expr(k)?;
                    self.expr(v)?;
                }
                self.b
                    .emit(Op::CallBuiltin(host::GMAP_LIT, (pairs.len() * 2) as u8), 0);
            }
            Expr::StructLit { type_name, fields } => self.struct_lit(type_name, fields)?,
            Expr::Make {
                is_map,
                len,
                elem_zero,
            } => {
                if *is_map {
                    let c = self.b.add_constant(Value::str("map"));
                    self.b.emit(Op::LoadConst(c), 0);
                    self.b.emit(Op::CallBuiltin(host::GMAKE, 1), 0);
                } else {
                    let c = self.b.add_constant(Value::str("slice"));
                    self.b.emit(Op::LoadConst(c), 0);
                    match len {
                        Some(e) => self.expr(e)?,
                        None => {
                            self.b.emit(Op::LoadInt(0), 0);
                        }
                    }
                    self.expr(elem_zero)?;
                    self.b.emit(Op::CallBuiltin(host::GMAKE, 3), 0);
                }
            }
            Expr::MakeChan { cap } => {
                match cap {
                    Some(e) => self.expr(e)?,
                    None => {
                        self.b.emit(Op::LoadInt(0), 0);
                    }
                }
                self.b.emit(Op::ChanMake, 0);
            }
            Expr::Recv { chan } => {
                self.expr(chan)?;
                self.b.emit(Op::ChanRecv, 0);
            }
            Expr::FuncLit { params, body, .. } => {
                self.emit_funclit(params, body);
            }
        }
        Ok(())
    }

    /// Lower a struct composite literal, filling every declared field in
    /// declaration order (keyed elements matched by name, positional by order,
    /// omitted fields defaulted to their type's zero value).
    fn struct_lit(
        &mut self,
        type_name: &str,
        given: &[(Option<String>, Expr)],
    ) -> Result<(), String> {
        let decl = self
            .struct_fields
            .get(type_name)
            .cloned()
            .ok_or_else(|| format!("go-rs: undefined struct type `{type_name}`"))?;
        let keyed = given.iter().any(|(k, _)| k.is_some());

        let tc = self.b.add_constant(Value::str(type_name.to_string()));
        self.b.emit(Op::LoadConst(tc), 0);
        for (i, (fname, fty)) in decl.iter().enumerate() {
            let fc = self.b.add_constant(Value::str(fname.clone()));
            self.b.emit(Op::LoadConst(fc), 0);
            let value: Option<&Expr> = if keyed {
                given
                    .iter()
                    .find(|(k, _)| k.as_deref() == Some(fname))
                    .map(|(_, v)| v)
            } else {
                given.get(i).map(|(_, v)| v)
            };
            match value {
                Some(e) => self.expr(e)?,
                None => self.emit_default(numtype_of_ty(fty), 0),
            }
        }
        self.b.emit(
            Op::CallBuiltin(host::GSTRUCT_NEW, (1 + decl.len() * 2) as u8),
            0,
        );
        Ok(())
    }

    /// Emit the right-hand side of a binding to `name`, additionally tracking
    /// when `name` becomes a statically-known closure (so a later `name(args)`
    /// dispatches directly).
    fn emit_rhs(&mut self, name: &str, e: &Expr) -> Result<(), String> {
        match e {
            Expr::FuncLit { params, body, .. } => {
                let id = self.emit_funclit(params, body);
                self.closure_vars.insert(name.to_string(), id);
            }
            Expr::Ident(src) if self.closure_vars.contains_key(src) => {
                let id = self.closure_vars[src];
                self.emit_value(e)?;
                self.closure_vars.insert(name.to_string(), id);
            }
            _ => {
                self.closure_vars.remove(name);
                self.emit_value(e)?;
            }
        }
        Ok(())
    }

    /// Emit `value`, then a `GSTRUCT_COPY` if its static type is a struct — Go
    /// copies a struct on assignment / parameter bind / return (slices and maps
    /// are reference types and pass through the copy unchanged).
    fn emit_value(&mut self, e: &Expr) -> Result<(), String> {
        self.expr(e)?;
        // Struct values are copied on assign/pass/return (Go value semantics) —
        // but `&x` is a pointer (a reference), so it is never copied.
        let is_pointer = matches!(e, Expr::Unary { op: UnOp::Addr, .. });
        if !is_pointer && self.structs.contains(&self.type_name(e)) {
            self.b.emit(Op::CallBuiltin(host::GSTRUCT_COPY, 1), 0);
        }
        Ok(())
    }

    /// Lower a method call `recv.method(args)`. The receiver's static type names
    /// the method set; the receiver is passed as the first (deepest) argument.
    /// Receivers use reference semantics (the receiver struct is not copied), so
    /// a method mutating a field is observed by the caller — matching Go's
    /// pointer-receiver idiom.
    fn method_call(
        &mut self,
        recv: &Expr,
        method: &str,
        args: &[Expr],
        line: u32,
    ) -> Result<(), String> {
        let ty = self.type_name(recv);

        // Static dispatch: the receiver's concrete struct type is known and
        // declares the method — a direct `Op::Call` to `T.method`.
        if let Some(&arity) = self.methods.get(&(ty.clone(), method.to_string())) {
            if arity != args.len() {
                return Err(format!(
                    "go-rs: `{ty}.{method}` takes {arity} argument(s), got {} (line {line})",
                    args.len()
                ));
            }
            self.expr(recv)?;
            for a in args {
                self.emit_value(a)?;
            }
            let idx = self.b.add_name(&format!("{ty}.{method}"));
            self.b.emit(Op::Call(idx, args.len() as u8 + 1), line);
            self.emit_panic_check(line);
            return Ok(());
        }

        // Dynamic dispatch (interface / unknown static type): a runtime
        // type-switch over every concrete type that implements `method` with a
        // matching arity, calling the one whose name matches the receiver's
        // runtime type. Every struct heap object carries its type name.
        let mut candidates: Vec<String> = self
            .methods
            .iter()
            .filter(|((_, m), &arity)| m == method && arity == args.len())
            .map(|((t, _), _)| t.clone())
            .collect();
        candidates.sort();
        if candidates.is_empty() {
            return Err(format!(
                "go-rs: no method `{method}` with {} argument(s) (line {line})",
                args.len()
            ));
        }

        let n = self.temp_counter;
        self.temp_counter += 1;
        let recv_tmp = format!("$mrecv{n}");
        let ty_tmp = format!("$mty{n}");
        self.expr(recv)?;
        self.emit_set(&recv_tmp, line);
        self.emit_get(&recv_tmp, line);
        self.b.emit(Op::CallBuiltin(host::GTYPEOF, 1), line);
        self.emit_set(&ty_tmp, line);

        let mut end_jumps = Vec::new();
        for t in &candidates {
            self.emit_get(&ty_tmp, line);
            let tc = self.b.add_constant(Value::str(t.clone()));
            self.b.emit(Op::LoadConst(tc), line);
            self.b.emit(Op::StrEq, line);
            let jf = self.b.emit(Op::JumpIfFalse(0), line);
            self.emit_get(&recv_tmp, line);
            for a in args {
                self.emit_value(a)?;
            }
            let idx = self.b.add_name(&format!("{t}.{method}"));
            self.b.emit(Op::Call(idx, args.len() as u8 + 1), line);
            end_jumps.push(self.b.emit(Op::Jump(0), line));
            let next = self.b.current_pos();
            self.b.patch_jump(jf, next);
        }
        // No concrete type matched — a nil interface call; yield nil.
        self.b.emit(Op::LoadUndef, line);
        let end = self.b.current_pos();
        for j in end_jumps {
            self.b.patch_jump(j, end);
        }
        self.emit_panic_check(line);
        Ok(())
    }

    /// The number of result values a call expression yields, if it targets a
    /// known top-level function (for multi-value-return destructuring).
    fn call_result_count(&self, e: &Expr) -> Option<usize> {
        if let Expr::Call { func, .. } = e {
            match func.as_ref() {
                Expr::Ident(name) => return self.funcs.get(name).map(|s| s.nresults),
                // A method call `recv.M()` — look up M's result count on the
                // receiver's static type. (A package call like `strings.Split`
                // has an untyped receiver, so this yields `None`.)
                Expr::Selector { recv, field } => {
                    let rt = self.type_name(recv);
                    if !rt.is_empty() {
                        return self.method_nresults.get(&(rt, field.clone())).copied();
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// The static Go type name of an expression, or `""` when unknown. Drives
    /// method dispatch and struct value-copy.
    fn type_name(&self, e: &Expr) -> String {
        match e {
            Expr::Ident(n) => self.decl_types.get(n).cloned().unwrap_or_default(),
            Expr::StructLit { type_name, .. } => type_name.clone(),
            // A type assertion `x.(T)` has static type T.
            Expr::TypeAssert { ty, .. } => base_type(ty),
            // `&x` / `*p` name the same type as their operand (a `*Point` handle
            // dispatches methods and reads fields like a `Point`).
            Expr::Unary {
                op: UnOp::Addr | UnOp::Deref,
                rhs,
            } => self.type_name(rhs),
            Expr::Selector { recv, field } => {
                // A field's declared type, looked up on the receiver's struct.
                let rt = self.type_name(recv);
                self.struct_fields
                    .get(&rt)
                    .and_then(|fs| fs.iter().find(|(n, _)| n == field))
                    .map(|(_, t)| base_type(t))
                    .unwrap_or_default()
            }
            Expr::Call { func, .. } => match func.as_ref() {
                Expr::Ident(name) => self
                    .funcs
                    .get(name)
                    .map(|s| base_type(&s.result_ty))
                    .unwrap_or_default(),
                _ => String::new(),
            },
            _ => String::new(),
        }
    }

    fn binary(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) -> Result<(), String> {
        // Short-circuit logical operators.
        match op {
            BinOp::And => {
                self.expr(lhs)?;
                let jf = self.b.emit(Op::JumpIfFalseKeep(0), 0);
                self.b.emit(Op::Pop, 0);
                self.expr(rhs)?;
                let end = self.b.current_pos();
                self.b.patch_jump(jf, end);
                return Ok(());
            }
            BinOp::Or => {
                self.expr(lhs)?;
                let jt = self.b.emit(Op::JumpIfTrueKeep(0), 0);
                self.b.emit(Op::Pop, 0);
                self.expr(rhs)?;
                let end = self.b.current_pos();
                self.b.patch_jump(jt, end);
                return Ok(());
            }
            _ => {}
        }

        // Comparisons pick string vs numeric ops from the operand types.
        if let Some(strcmp) = str_compare_op(op) {
            let is_str = self.infer(lhs) == NumType::Str || self.infer(rhs) == NumType::Str;
            self.expr(lhs)?;
            self.expr(rhs)?;
            self.b
                .emit(if is_str { strcmp } else { num_compare_op(op) }, 0);
            return Ok(());
        }

        // Arithmetic.
        let l = self.infer(lhs);
        let r = self.infer(rhs);
        self.expr(lhs)?;
        self.expr(rhs)?;
        self.emit_arith(op, l, r, 0);
        Ok(())
    }

    /// Emit an arithmetic op for two already-pushed operands, appending
    /// `TruncInt` for integer division (Go truncates `int / int` toward zero).
    fn emit_arith(&mut self, op: BinOp, l: NumType, r: NumType, line: u32) {
        match op {
            BinOp::Add => {
                self.b.emit(Op::Add, line);
            }
            BinOp::Sub => {
                self.b.emit(Op::Sub, line);
            }
            BinOp::Mul => {
                self.b.emit(Op::Mul, line);
            }
            // `%` is integer-only in Go; route through the builtin so a zero
            // divisor panics (`runtime error: integer divide by zero`).
            BinOp::Mod => {
                self.b.emit(Op::CallBuiltin(host::GIMOD, 2), line);
                self.emit_panic_check(line);
            }
            BinOp::Div => {
                if l == NumType::Int && r == NumType::Int {
                    // Integer division panics on a zero divisor (and truncates
                    // toward zero, which GIDIV does).
                    self.b.emit(Op::CallBuiltin(host::GIDIV, 2), line);
                    self.emit_panic_check(line);
                } else {
                    // Float division: `x / 0.0` yields ±Inf like Go, no panic.
                    self.b.emit(Op::Div, line);
                }
            }
            // Bitwise operators (integer-only in Go).
            BinOp::BitAnd => {
                self.b.emit(Op::BitAnd, line);
            }
            BinOp::BitOr => {
                self.b.emit(Op::BitOr, line);
            }
            BinOp::BitXor => {
                self.b.emit(Op::BitXor, line);
            }
            BinOp::Shl => {
                self.b.emit(Op::Shl, line);
            }
            BinOp::Shr => {
                self.b.emit(Op::Shr, line);
            }
            // `a &^ b` (bit clear) is `a & (^b)`.
            BinOp::AndNot => {
                self.b.emit(Op::BitNot, line);
                self.b.emit(Op::BitAnd, line);
            }
            other => unreachable!("emit_arith on non-arithmetic op {other:?}"),
        };
    }

    fn call(&mut self, func: &Expr, args: &[Expr], spread: bool, line: u32) -> Result<(), String> {
        // Multi-value spread: `f(g())` where `g` returns N>1 values passes them
        // as N arguments. Evaluate `g` into a tuple, extract each element into a
        // temporary, and recurse with those temporaries as the arguments.
        if args.len() == 1 && !spread {
            if let Some(n) = self.call_result_count(&args[0]) {
                if n >= 2 {
                    let base = self.temp_counter;
                    self.temp_counter += 1;
                    let tup = format!("$sp{base}");
                    self.expr(&args[0])?;
                    self.emit_set(&tup, line);
                    let mut expanded = Vec::with_capacity(n);
                    for i in 0..n {
                        self.emit_get(&tup, line);
                        self.b.emit(Op::LoadInt(i as i64), line);
                        self.b.emit(Op::CallBuiltin(host::GINDEX_GET, 2), line);
                        let t = format!("$spv{base}_{i}");
                        self.types.insert(t.clone(), NumType::Unknown);
                        self.decl_types.insert(t.clone(), String::new());
                        self.emit_set(&t, line);
                        expanded.push(Expr::Ident(t));
                    }
                    return self.call(func, &expanded, false, line);
                }
            }
        }
        // An immediately-invoked function literal: `func(...){...}(args)`.
        if let Expr::FuncLit { params, body, .. } = func {
            let id = self.emit_funclit(params, body);
            self.emit_closure_call(id, args, line)?;
            return Ok(());
        }
        if let Expr::Selector { recv, field } = func {
            if let Expr::Ident(pkg) = recv.as_ref() {
                // `fmt.*` print family.
                if pkg == "fmt" {
                    // `fmt.Errorf(f, args...)` builds a real error value:
                    // `&$errorString{s: fmt.Sprintf(f, args...)}` (the error type
                    // synthesized by the linker when Errorf is used). %w wrapping
                    // is not modeled — the verb formats like %v.
                    if field == "Errorf" {
                        let msg = Expr::Call {
                            func: Box::new(Expr::Selector {
                                recv: Box::new(Expr::Ident("fmt".to_string())),
                                field: "Sprintf".to_string(),
                            }),
                            args: args.to_vec(),
                            spread: false,
                            line,
                        };
                        let lit = Expr::StructLit {
                            type_name: "$errorString".to_string(),
                            fields: vec![(Some("s".to_string()), msg)],
                        };
                        let addr = Expr::Unary {
                            op: UnOp::Addr,
                            rhs: Box::new(lit),
                        };
                        return self.expr(&addr);
                    }
                    let id = match field.as_str() {
                        "Println" => host::GPRINTLN,
                        "Print" => host::GPRINT,
                        "Printf" => host::GPRINTF,
                        "Sprintf" => host::GSPRINTF,
                        "Sprint" => host::GSPRINT,
                        "Sprintln" => host::GSPRINTLN,
                        _ => {
                            return Err(format!(
                                "go-rs: unsupported call `fmt.{field}` (line {line})"
                            ))
                        }
                    };
                    // A value implementing `error`/`Stringer` prints via its
                    // method; `$stringify` (synthesized when such a type exists)
                    // does that at runtime and passes other values through.
                    let has_stringify = self.funcs.contains_key("$stringify");
                    for a in args {
                        if has_stringify {
                            self.call(
                                &Expr::Ident("$stringify".to_string()),
                                std::slice::from_ref(a),
                                false,
                                line,
                            )?;
                        } else {
                            self.expr(a)?;
                        }
                    }
                    self.b.emit(Op::CallBuiltin(id, args.len() as u8), line);
                    return Ok(());
                }
                // Standard-library package calls.
                if matches!(pkg.as_str(), "strings" | "strconv" | "math" | "sort" | "os") {
                    let id = host::stdlib::resolve(pkg, field).ok_or_else(|| {
                        format!("go-rs: unsupported call `{pkg}.{field}` (line {line})")
                    })?;
                    for a in args {
                        self.expr(a)?;
                    }
                    self.b.emit(Op::CallBuiltin(id, args.len() as u8), line);
                    return Ok(());
                }
            }
            // Otherwise a method call `recv.method(args)`.
            return self.method_call(recv, field, args, line);
        }

        // Bare-name call: a language builtin or a user function.
        if let Expr::Ident(name) = func {
            // A type conversion `T(x)` — a builtin numeric/string/bool type name
            // applied to a single value.
            if args.len() == 1 && is_conversion_type(name) {
                self.expr(&args[0])?;
                let c = self.b.add_constant(Value::str(name.clone()));
                self.b.emit(Op::LoadConst(c), line);
                self.b.emit(Op::CallBuiltin(host::GCONV, 2), line);
                return Ok(());
            }
            // `panic(v)` records the panic then unwinds to the function's defer
            // drain (jump patched to the panic epilogue).
            if name == "panic" {
                for a in args {
                    self.emit_value(a)?;
                }
                self.b
                    .emit(Op::CallBuiltin(host::GPANIC, args.len() as u8), line);
                let j = self.b.emit(Op::Jump(0), line);
                self.panic_jumps.push(j);
                return Ok(());
            }
            // `recover()` returns the in-flight panic value (or nil) and stops it.
            if name == "recover" {
                self.b.emit(Op::CallBuiltin(host::GRECOVER, 0), line);
                return Ok(());
            }
            // `close(ch)` lowers to the channel-close op, not a builtin.
            if name == "close" {
                for a in args {
                    self.expr(a)?;
                }
                self.b.emit(Op::ChanClose, line);
                // `close` is a statement; leave a value so ExprStmt's Pop is
                // balanced (the op consumes the channel and pushes nothing, so
                // synthesize an Undef result).
                self.b.emit(Op::LoadUndef, line);
                return Ok(());
            }
            // `append(base, xs...)` — spread every element of the slice `xs`
            // into the result (not append the slice as a single element).
            if name == "append" && spread {
                for a in args {
                    self.expr(a)?;
                }
                self.b.emit(
                    Op::CallBuiltin(host::GAPPEND_SPREAD, args.len() as u8),
                    line,
                );
                return Ok(());
            }
            // Builtins that take a variable arg count.
            let simple_builtin = match name.as_str() {
                "__rust_compile" => Some(host::GFFI_COMPILE),
                "println" => Some(host::GEPRINTLN),
                "print" => Some(host::GEPRINT),
                "len" => Some(host::GLEN),
                "cap" => Some(host::GCAP),
                "append" => Some(host::GAPPEND),
                "delete" => Some(host::GDELETE),
                "copy" => Some(host::GCOPY),
                // Go 1.21 ordered builtins.
                "min" => Some(host::GMIN),
                "max" => Some(host::GMAX),
                _ => None,
            };
            if let Some(id) = simple_builtin {
                for a in args {
                    self.expr(a)?;
                }
                self.b.emit(Op::CallBuiltin(id, args.len() as u8), line);
                return Ok(());
            }
            // A variable statically known to hold a closure — dispatch directly.
            if let Some(&id) = self.closure_vars.get(name) {
                self.emit_get(name, line);
                self.emit_closure_call(id, args, line)?;
                return Ok(());
            }
            // A function value held in a variable — a func-typed parameter, a
            // captured func value inside a lambda, or a local bound to a closure
            // whose concrete target isn't known statically. Dispatch through the
            // closure's stored subroutine name-index via `Op::CallDynamic`.
            let is_value_call = self.active_captures.contains_key(name)
                || self.scope.as_ref().is_some_and(|s| s.has(name))
                || self
                    .decl_types
                    .get(name)
                    .is_some_and(|t| t.starts_with("func"))
                // A declared variable (e.g. a global in `main` bound to a func
                // value via `:=` or a multi-return destructure) that isn't a
                // top-level function — dispatch its value dynamically.
                || (self.types.contains_key(name) && !self.funcs.contains_key(name));
            if is_value_call {
                let n = self.temp_counter;
                self.temp_counter += 1;
                let cv = format!("$dc{n}");
                let ni = format!("$dni{n}");
                // Stash the closure value, then read its name-index.
                self.emit_get(name, line);
                self.emit_set(&cv, line);
                self.emit_get(&cv, line);
                self.b
                    .emit(Op::CallBuiltin(host::GCLOSURE_NAMEIDX, 1), line);
                self.emit_set(&ni, line);
                // Push self (the closure), the args, then the name-index.
                self.emit_get(&cv, line);
                for a in args {
                    self.emit_value(a)?;
                }
                self.emit_get(&ni, line);
                self.b.emit(Op::CallDynamic(args.len() as u8 + 1), line);
                self.emit_panic_check(line);
                return Ok(());
            }
            if let Some(sig) = self.funcs.get(name) {
                let variadic = sig.variadic;
                let arity = sig.arity;
                if variadic {
                    // Fixed params come first; the trailing arguments are packed
                    // into the variadic slice parameter (or, for `f(xs...)`, the
                    // already-a-slice argument is passed directly).
                    let fixed = arity - 1;
                    if args.len() < fixed {
                        return Err(format!(
                            "go-rs: `{name}` needs at least {fixed} argument(s), got {} (line {line})",
                            args.len()
                        ));
                    }
                    for a in &args[..fixed] {
                        self.emit_value(a)?;
                    }
                    if spread {
                        // `f(a, xs...)` — the last argument is the slice itself.
                        self.emit_value(&args[fixed])?;
                    } else {
                        // Pack the remaining arguments into a fresh slice.
                        let rest = &args[fixed..];
                        for a in rest {
                            self.emit_value(a)?;
                        }
                        self.b
                            .emit(Op::CallBuiltin(host::GSLICE_LIT, rest.len() as u8), line);
                    }
                    let idx = self.b.add_name(name);
                    self.b.emit(Op::Call(idx, arity as u8), line);
                    self.emit_panic_check(line);
                    return Ok(());
                }
                if arity != args.len() {
                    return Err(format!(
                        "go-rs: `{name}` takes {arity} argument(s), got {} (line {line})",
                        args.len()
                    ));
                }
                for a in args {
                    self.emit_value(a)?;
                }
                let idx = self.b.add_name(name);
                self.b.emit(Op::Call(idx, args.len() as u8), line);
                self.emit_panic_check(line);
                return Ok(());
            }
            // With an inline `rust {}` block present, an otherwise-unresolved
            // bare name may be an FFI export — dispatch it by name at runtime.
            if self.has_ffi {
                for a in args {
                    self.expr(a)?;
                }
                let c = self.b.add_constant(Value::str(name.clone()));
                self.b.emit(Op::LoadConst(c), line);
                self.b
                    .emit(Op::CallBuiltin(host::GFFI_CALL, args.len() as u8 + 1), line);
                return Ok(());
            }
            return Err(format!("go-rs: undefined: {name} (line {line})"));
        }

        // Any other callee expression yields a function value at runtime — e.g.
        // an element of a slice/map of funcs (`fns[i](x)`), a field holding a
        // closure, or the result of another call. Evaluate it and dispatch
        // dynamically through the closure's stored subroutine name-index.
        self.call_value(func, args, line)
    }

    /// Call a function *value* produced by an arbitrary expression: stash it,
    /// read its subroutine name-index, then push `self`, the args, and the
    /// name-index and issue `Op::CallDynamic`.
    fn call_value(&mut self, func: &Expr, args: &[Expr], line: u32) -> Result<(), String> {
        let n = self.temp_counter;
        self.temp_counter += 1;
        let cv = format!("$cv{n}");
        let ni = format!("$cni{n}");
        self.expr(func)?;
        self.emit_set(&cv, line);
        self.emit_get(&cv, line);
        self.b
            .emit(Op::CallBuiltin(host::GCLOSURE_NAMEIDX, 1), line);
        self.emit_set(&ni, line);
        self.emit_get(&cv, line); // self (the closure)
        for a in args {
            self.emit_value(a)?;
        }
        self.emit_get(&ni, line);
        self.b.emit(Op::CallDynamic(args.len() as u8 + 1), line);
        self.emit_panic_check(line);
        Ok(())
    }

    // ── static type inference ──────────────────────────────────────────────

    fn infer(&self, e: &Expr) -> NumType {
        match e {
            Expr::Int(_) => NumType::Int,
            Expr::Float(..) => NumType::Float,
            Expr::Str(_) => NumType::Str,
            Expr::Bool(_) => NumType::Bool,
            Expr::Ident(name) => self.types.get(name).copied().unwrap_or(NumType::Unknown),
            Expr::Unary { op, rhs } => match op {
                UnOp::Neg => self.infer(rhs),
                UnOp::Not => NumType::Bool,
                UnOp::BitNot => NumType::Int,
                // `&x` / `*p` carry the operand's category (a struct handle stays
                // a struct handle).
                UnOp::Addr | UnOp::Deref => self.infer(rhs),
            },
            Expr::Binary { op, lhs, rhs } => match op {
                BinOp::And
                | BinOp::Or
                | BinOp::Eq
                | BinOp::Ne
                | BinOp::Lt
                | BinOp::Gt
                | BinOp::Le
                | BinOp::Ge => NumType::Bool,
                // Bitwise/shift operators are integer-typed.
                BinOp::BitAnd
                | BinOp::BitOr
                | BinOp::BitXor
                | BinOp::Shl
                | BinOp::Shr
                | BinOp::AndNot => NumType::Int,
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                    let l = self.infer(lhs);
                    let r = self.infer(rhs);
                    if *op == BinOp::Add && (l == NumType::Str || r == NumType::Str) {
                        NumType::Str
                    } else if l == NumType::Float || r == NumType::Float {
                        NumType::Float
                    } else if l == NumType::Int && r == NumType::Int {
                        NumType::Int
                    } else {
                        NumType::Unknown
                    }
                }
            },
            Expr::Call { func, args, .. } => match func.as_ref() {
                Expr::Ident(name) => match name.as_str() {
                    "len" | "cap" | "copy" => NumType::Int,
                    // A conversion `T(x)` is typed as T.
                    n if args.len() == 1 && is_conversion_type(n) => numtype_of_ty(n),
                    _ => self
                        .funcs
                        .get(name)
                        .map(|s| s.result)
                        .unwrap_or(NumType::Unknown),
                },
                // A method call's result type is not tracked numerically yet.
                Expr::Selector { .. } => {
                    let _ = args;
                    NumType::Unknown
                }
                _ => NumType::Unknown,
            },
            // A struct field's numeric category, when the field type is known.
            Expr::Selector { recv, field } => {
                let rt = self.type_name(recv);
                self.struct_fields
                    .get(&rt)
                    .and_then(|fs| fs.iter().find(|(n, _)| n == field))
                    .map(|(_, t)| numtype_of_ty(t))
                    .unwrap_or(NumType::Unknown)
            }
            // Composite literals, indexing, make, and channel ops have no
            // known numeric category.
            // A type assertion `x.(T)` is typed as T.
            Expr::TypeAssert { ty, .. } => numtype_of_ty(ty),
            Expr::Index { .. }
            | Expr::Slice { .. }
            | Expr::SliceLit { .. }
            | Expr::MapLit { .. }
            | Expr::StructLit { .. }
            | Expr::Make { .. }
            | Expr::MakeChan { .. }
            | Expr::Recv { .. }
            | Expr::FuncLit { .. } => NumType::Unknown,
        }
    }
}

/// The fusevm subroutine name for a function or method. A method on receiver
/// type `T` (or `*T`) is named `T.method`; a plain function keeps its own name.
fn sub_name(f: &Func) -> String {
    match &f.receiver {
        Some(r) => format!("{}.{}", base_type(&r.ty), f.name),
        None => f.name.clone(),
    }
}

/// The base type name of a type string: strips a leading pointer `*`, so a
/// value receiver `T` and pointer receiver `*T` mangle to the same method set.
fn base_type(ty: &str) -> String {
    ty.trim_start_matches('*').to_string()
}

/// Fold a compile-time-constant float expression to a single `f64`, evaluated
/// with exact rational arithmetic and rounded once — matching Go's
/// arbitrary-precision constant semantics (where go-rs's runtime `f64` would
/// double-round). Returns `None` for a non-constant expression or when the exact
/// values leave the range where `f64` conversion is exact, so the caller falls
/// back to ordinary runtime evaluation.
fn fold_const_float(e: &Expr) -> Option<f64> {
    let (num, den) = fold_rational(e)?;
    rational_to_f64(num, den)
}

/// Evaluate a constant numeric expression to an exact rational `(num, den)`,
/// `den > 0`. `None` if it references a variable/call or overflows `i128`.
fn fold_rational(e: &Expr) -> Option<(i128, i128)> {
    match e {
        Expr::Int(n) => Some((*n as i128, 1)),
        Expr::Float(_, Some((mant, scale))) => {
            if *scale >= 0 {
                Some((*mant, pow10(*scale as u32)?))
            } else {
                Some((mant.checked_mul(pow10((-scale) as u32)?)?, 1))
            }
        }
        Expr::Float(_, None) => None,
        Expr::Unary { op: UnOp::Neg, rhs } => {
            let (a, b) = fold_rational(rhs)?;
            Some((a.checked_neg()?, b))
        }
        Expr::Binary { op, lhs, rhs } => {
            let (an, ad) = fold_rational(lhs)?;
            let (bn, bd) = fold_rational(rhs)?;
            let (n, d) = match op {
                // a/b ± c/d = (a·d ± c·b) / (b·d)
                BinOp::Add => (
                    an.checked_mul(bd)?.checked_add(bn.checked_mul(ad)?)?,
                    ad.checked_mul(bd)?,
                ),
                BinOp::Sub => (
                    an.checked_mul(bd)?.checked_sub(bn.checked_mul(ad)?)?,
                    ad.checked_mul(bd)?,
                ),
                BinOp::Mul => (an.checked_mul(bn)?, ad.checked_mul(bd)?),
                BinOp::Div => {
                    if bn == 0 {
                        return None;
                    }
                    (an.checked_mul(bd)?, ad.checked_mul(bn)?)
                }
                _ => return None,
            };
            Some(reduce(n, d))
        }
        _ => None,
    }
}

/// `10^n` as `i128`, or `None` on overflow.
fn pow10(n: u32) -> Option<i128> {
    10i128.checked_pow(n)
}

/// Reduce a rational to lowest terms with a positive denominator.
fn reduce(mut n: i128, mut d: i128) -> (i128, i128) {
    if d < 0 {
        n = -n;
        d = -d;
    }
    let g = gcd(n.unsigned_abs(), d.unsigned_abs()) as i128;
    if g > 1 {
        (n / g, d / g)
    } else {
        (n, d)
    }
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}

/// Convert an exact rational to the nearest `f64`, but only when both terms are
/// exactly representable (`< 2^53`) so the single IEEE division is correctly
/// rounded — Go's round-once behavior. Otherwise `None` (fall back to runtime).
fn rational_to_f64(num: i128, den: i128) -> Option<f64> {
    const LIMIT: i128 = 1 << 53;
    if num.abs() < LIMIT && den.abs() < LIMIT {
        Some(num as f64 / den as f64)
    } else {
        None
    }
}

/// Whether `name` is an imported package go-rs dispatches by name (so a `defer
/// pkg.Fn(...)` needn't snapshot the callee).
fn is_package(name: &str) -> bool {
    matches!(name, "fmt" | "strings" | "strconv" | "math" | "sort" | "os")
}

/// Normalize a written type to the runtime tag [`host::GTYPETAG`] produces:
/// pointers/named types → the base name, `[]T` → `[]`, `map[..]` → `map`,
/// `func…` → `func`, and interface types (`any`, `interface{…}`, `error`) → `""`
/// (which matches any value).
fn type_to_tag(ty: &str) -> String {
    let ty = ty.trim_start_matches('*');
    if ty.starts_with("[]") {
        "[]".to_string()
    } else if ty.starts_with("map[") {
        "map".to_string()
    } else if ty.starts_with("func") {
        "func".to_string()
    } else if ty == "any" || ty == "interface{}" || ty == "interface{ }" || ty == "error" {
        String::new()
    } else {
        ty.to_string()
    }
}

/// Whether `name` is a builtin type usable as a conversion `T(x)`.
fn is_conversion_type(name: &str) -> bool {
    matches!(
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
            | "byte"
            | "rune"
            | "float32"
            | "float64"
            | "string"
            | "bool"
            // Slice conversions from a string: `[]byte(s)` / `[]rune(s)`.
            | "[]byte"
            | "[]rune"
    )
}

/// Whether `name` is a predeclared builtin call (referenced by name, not a value).
fn is_builtin_call(name: &str) -> bool {
    matches!(
        name,
        "len" | "cap" | "append" | "delete" | "copy" | "make" | "min" | "max" | "println" | "print"
    )
}

fn assign_binop(op: AssignOp) -> BinOp {
    match op {
        AssignOp::Add => BinOp::Add,
        AssignOp::Sub => BinOp::Sub,
        AssignOp::Mul => BinOp::Mul,
        AssignOp::Div => BinOp::Div,
        AssignOp::Mod => BinOp::Mod,
        AssignOp::BitAnd => BinOp::BitAnd,
        AssignOp::BitOr => BinOp::BitOr,
        AssignOp::BitXor => BinOp::BitXor,
        AssignOp::Shl => BinOp::Shl,
        AssignOp::Shr => BinOp::Shr,
        AssignOp::AndNot => BinOp::AndNot,
        AssignOp::Set => unreachable!("plain `=` is not an arithmetic assignment"),
    }
}

/// The string-comparison op for a comparison operator, or `None` if `op` is not
/// a comparison.
fn str_compare_op(op: BinOp) -> Option<Op> {
    Some(match op {
        BinOp::Eq => Op::StrEq,
        BinOp::Ne => Op::StrNe,
        BinOp::Lt => Op::StrLt,
        BinOp::Gt => Op::StrGt,
        BinOp::Le => Op::StrLe,
        BinOp::Ge => Op::StrGe,
        _ => return None,
    })
}

/// The numeric-comparison op paired with [`str_compare_op`].
fn num_compare_op(op: BinOp) -> Op {
    match op {
        BinOp::Eq => Op::NumEq,
        BinOp::Ne => Op::NumNe,
        BinOp::Lt => Op::NumLt,
        BinOp::Gt => Op::NumGt,
        BinOp::Le => Op::NumLe,
        BinOp::Ge => Op::NumGe,
        _ => unreachable!("num_compare_op on non-comparison op"),
    }
}

/// The source line a statement reports for the `--dap` marker, or 0 for
/// statements that carry no line of their own (blocks, bare expressions).
fn stmt_line(s: &Stmt) -> u32 {
    match s {
        Stmt::Var { line, .. }
        | Stmt::Short { line, .. }
        | Stmt::Assign { line, .. }
        | Stmt::AssignMulti { line, .. }
        | Stmt::IncDec { line, .. }
        | Stmt::Return(_, line)
        | Stmt::If { line, .. }
        | Stmt::For { line, .. }
        | Stmt::ForRange { line, .. }
        | Stmt::Go { line, .. }
        | Stmt::Defer { line, .. }
        | Stmt::Send { line, .. }
        | Stmt::Select { line, .. }
        | Stmt::Switch { line, .. }
        | Stmt::TypeSwitch { line, .. }
        | Stmt::Fallthrough(line)
        | Stmt::Break(line)
        | Stmt::Continue(line) => *line,
        Stmt::ExprStmt(_) | Stmt::Block(_) => 0,
    }
}

/// True if any statement in `body` (recursively) evaluates a `__rust_compile`
/// call — the desugar target of an inline `rust {}` block.
fn body_has_ffi(body: &[Stmt]) -> bool {
    body.iter().any(|s| match s {
        Stmt::Var { init, .. } => init.as_ref().is_some_and(expr_has_ffi),
        Stmt::Short { values, .. } => values.iter().any(expr_has_ffi),
        Stmt::Assign { value, .. } => expr_has_ffi(value),
        Stmt::AssignMulti { values, .. } => values.iter().any(expr_has_ffi),
        Stmt::ExprStmt(e) => expr_has_ffi(e),
        Stmt::Return(vs, _) => vs.iter().any(expr_has_ffi),
        Stmt::If { then, els, .. } => body_has_ffi(then) || body_has_ffi(els),
        Stmt::For { body, .. } | Stmt::ForRange { body, .. } | Stmt::Block(body) => {
            body_has_ffi(body)
        }
        Stmt::Go { call, .. } | Stmt::Defer { call, .. } => expr_has_ffi(call),
        Stmt::Send { chan, val, .. } => expr_has_ffi(chan) || expr_has_ffi(val),
        Stmt::Select { cases, default, .. } => {
            cases.iter().any(|c| body_has_ffi(&c.body))
                || default.as_ref().is_some_and(|d| body_has_ffi(d))
        }
        Stmt::Switch { cases, default, .. } => {
            cases.iter().any(|c| body_has_ffi(&c.body))
                || default.as_ref().is_some_and(|d| body_has_ffi(d))
        }
        Stmt::TypeSwitch {
            init,
            expr,
            cases,
            default,
            ..
        } => {
            init.as_ref()
                .is_some_and(|s| body_has_ffi(std::slice::from_ref(s)))
                || expr_has_ffi(expr)
                || cases.iter().any(|c| body_has_ffi(&c.body))
                || default.as_ref().is_some_and(|d| body_has_ffi(d))
        }
        Stmt::IncDec { .. } | Stmt::Break(_) | Stmt::Continue(_) | Stmt::Fallthrough(_) => false,
    })
}

/// True if `e` contains a `__rust_compile(...)` call.
fn expr_has_ffi(e: &Expr) -> bool {
    match e {
        Expr::Call { func, args, .. } => {
            matches!(func.as_ref(), Expr::Ident(n) if n == "__rust_compile")
                || args.iter().any(expr_has_ffi)
        }
        Expr::Unary { rhs, .. } => expr_has_ffi(rhs),
        Expr::Binary { lhs, rhs, .. } => expr_has_ffi(lhs) || expr_has_ffi(rhs),
        _ => false,
    }
}
