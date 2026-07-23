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

/// Back-patch targets for one enclosing `for` loop.
#[derive(Default)]
struct LoopScope {
    breaks: Vec<usize>,
    continues: Vec<usize>,
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
    /// Struct type names declared with `type T struct`.
    structs: HashSet<String>,
    /// Each struct type's fields, in declaration order: `(name, type)`.
    struct_fields: HashMap<String, Vec<(String, String)>>,
    /// Method arities keyed by `(receiver type, method name)`.
    methods: HashMap<(String, String), usize>,
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
            Stmt::IncDec { target, .. } => ex(target),
            Stmt::ExprStmt(e) => ex(e),
            Stmt::Return(vs, _) => vs.iter().any(ex),
            Stmt::If {
                then, els, cond, ..
            } => ex(cond) || body_uses_panic(then) || body_uses_panic(els),
            Stmt::For { body, .. } | Stmt::ForRange { body, .. } => body_uses_panic(body),
            Stmt::Block(b) => body_uses_panic(b),
            Stmt::Go { call, .. } | Stmt::Defer { call, .. } => ex(call),
            Stmt::Send { chan, val, .. } => ex(chan) || ex(val),
            Stmt::Select { cases, default, .. } => {
                cases.iter().any(|c| body_uses_panic(&c.body))
                    || default.as_deref().is_some_and(body_uses_panic)
            }
            Stmt::Break(_) | Stmt::Continue(_) => false,
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
        Stmt::Block(b) => b.iter().for_each(|s| free_stmt(s, bound, out)),
        Stmt::Break(_) | Stmt::Continue(_) => {}
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
        Stmt::Block(b) => b.iter().for_each(|s| walk_stmt_exprs(s, f)),
        Stmt::Break(_) | Stmt::Continue(_) => {}
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
    for f in &prog.funcs {
        match &f.receiver {
            Some(r) => {
                methods.insert((base_type(&r.ty), f.name.clone()), f.params.len());
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
                    },
                );
            }
        }
    }

    let has_ffi = body_has_ffi(&prog.main) || prog.funcs.iter().any(|f| body_has_ffi(&f.body));
    let mut c = Compiler {
        b: ChunkBuilder::new(),
        scope: None,
        types: HashMap::new(),
        decl_types: HashMap::new(),
        funcs,
        structs,
        struct_fields,
        methods,
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
    };

    // ── main body (global scope) ──
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

        // Params/locals captured by a nested closure are boxed (shared cells).
        let mut all_params: Vec<Param> = Vec::new();
        if let Some(r) = &f.receiver {
            all_params.push(r.clone());
        }
        all_params.extend(f.params.iter().cloned());
        self.boxed = boxed_vars(&all_params, &f.body);

        // Prologue: pop args into their slots. The last argument is on top of
        // the stack, so bind slots high-to-low (receiver deepest, at slot 0).
        for i in (0..slot).rev() {
            self.b.emit(Op::SetSlot(i), f.line);
        }
        self.box_params(&all_params);

        self.fn_has_defer = body_has_defer(&f.body);
        self.panic_jumps.clear();
        if self.fn_has_defer {
            self.b.emit(Op::CallBuiltin(host::GDEFER_ENTER, 0), f.line);
            self.b.emit(Op::Pop, f.line);
        }

        for s in &f.body {
            self.stmt(s)?;
        }
        // Fall-off: a function with no explicit `return` yields nil.
        self.b.emit(Op::LoadUndef, f.line);
        self.emit_return(f.line);
        self.emit_panic_epilogue(&f.results, f.line);

        self.fn_has_defer = false;
        self.boxed = HashSet::new();
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
        // Return the function's zero values (int→0, string→"", …) so a recovered
        // call still yields the right shape and values. (Deferred mutation of
        // *named* results is a documented gap pending capture-by-reference.)
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
            Stmt::Block(b) => {
                for s in b {
                    self.fv_stmt(s, bound, caps);
                }
            }
            Stmt::Break(_) | Stmt::Continue(_) => {}
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
            Expr::Index { recv, index } => {
                self.fv_expr(recv, bound, caps);
                self.fv_expr(index, bound, caps);
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
            Expr::Int(_) | Expr::Float(_) | Expr::Str(_) | Expr::Bool(_) => {}
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

    /// Store the top of stack into a variable's raw storage (slot/global).
    fn emit_set_raw(&mut self, name: &str, line: u32) {
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
                match init {
                    Some(e) => self.emit_rhs(name, e)?,
                    // `var s T` where T is a struct → its zero value is a struct
                    // with every field zeroed, not nil (so `s.f` reads/writes and
                    // methods work). Other types use the scalar/nil default.
                    None if self.structs.contains(&decl_ty) => self.struct_lit(&decl_ty, &[])?,
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
                self.loops
                    .last_mut()
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
        // val := $it[key]  — index by the current key value
        if let Some(v) = val {
            self.emit_get(&it, 0);
            self.emit_get(&keys, 0);
            self.emit_get(&i, 0);
            self.b.emit(Op::CallBuiltin(host::GINDEX_GET, 2), 0);
            self.b.emit(Op::CallBuiltin(host::GINDEX_GET, 2), 0);
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
            Expr::Float(f) => {
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
                self.expr(rhs)?;
                self.b.emit(
                    match op {
                        UnOp::Neg => Op::Negate,
                        UnOp::Not => Op::LogNot,
                    },
                    0,
                );
            }
            Expr::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs)?,
            Expr::Call { func, args, line } => self.call(func, args, *line)?,
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
            }
            Expr::Index { recv, index } => {
                self.expr(recv)?;
                self.expr(index)?;
                self.b.emit(Op::CallBuiltin(host::GINDEX_GET, 2), 0);
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
        if self.structs.contains(&self.type_name(e)) {
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
            if let Expr::Ident(name) = func.as_ref() {
                return self.funcs.get(name).map(|s| s.nresults);
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
            BinOp::Add => self.b.emit(Op::Add, line),
            BinOp::Sub => self.b.emit(Op::Sub, line),
            BinOp::Mul => self.b.emit(Op::Mul, line),
            BinOp::Mod => self.b.emit(Op::Mod, line),
            BinOp::Div => {
                self.b.emit(Op::Div, line);
                if l == NumType::Int && r == NumType::Int {
                    self.b.emit(Op::TruncInt, line)
                } else {
                    0
                }
            }
            other => unreachable!("emit_arith on non-arithmetic op {other:?}"),
        };
    }

    fn call(&mut self, func: &Expr, args: &[Expr], line: u32) -> Result<(), String> {
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
                    let id = match field.as_str() {
                        "Println" => host::GPRINTLN,
                        "Print" => host::GPRINT,
                        "Printf" => host::GPRINTF,
                        _ => {
                            return Err(format!(
                                "go-rs: unsupported call `fmt.{field}` (line {line})"
                            ))
                        }
                    };
                    for a in args {
                        self.expr(a)?;
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
            // Builtins that take a variable arg count.
            let simple_builtin = match name.as_str() {
                "__rust_compile" => Some(host::GFFI_COMPILE),
                "println" => Some(host::GEPRINTLN),
                "print" => Some(host::GEPRINT),
                "len" => Some(host::GLEN),
                "cap" => Some(host::GCAP),
                "append" => Some(host::GAPPEND),
                "delete" => Some(host::GDELETE),
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
                if sig.arity != args.len() {
                    return Err(format!(
                        "go-rs: `{name}` takes {} argument(s), got {} (line {line})",
                        sig.arity,
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

        Err(format!("go-rs: cannot call this expression (line {line})"))
    }

    // ── static type inference ──────────────────────────────────────────────

    fn infer(&self, e: &Expr) -> NumType {
        match e {
            Expr::Int(_) => NumType::Int,
            Expr::Float(_) => NumType::Float,
            Expr::Str(_) => NumType::Str,
            Expr::Bool(_) => NumType::Bool,
            Expr::Ident(name) => self.types.get(name).copied().unwrap_or(NumType::Unknown),
            Expr::Unary { op, rhs } => match op {
                UnOp::Neg => self.infer(rhs),
                UnOp::Not => NumType::Bool,
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
                    "len" | "cap" => NumType::Int,
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
            Expr::Index { .. }
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

/// Whether `name` is an imported package go-rs dispatches by name (so a `defer
/// pkg.Fn(...)` needn't snapshot the callee).
fn is_package(name: &str) -> bool {
    matches!(name, "fmt" | "strings" | "strconv" | "math" | "sort" | "os")
}

/// Whether `name` is a predeclared builtin call (referenced by name, not a value).
fn is_builtin_call(name: &str) -> bool {
    matches!(
        name,
        "len" | "cap" | "append" | "delete" | "make" | "min" | "max" | "println" | "print"
    )
}

fn assign_binop(op: AssignOp) -> BinOp {
    match op {
        AssignOp::Add => BinOp::Add,
        AssignOp::Sub => BinOp::Sub,
        AssignOp::Mul => BinOp::Mul,
        AssignOp::Div => BinOp::Div,
        AssignOp::Mod => BinOp::Mod,
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
        | Stmt::IncDec { line, .. }
        | Stmt::Return(_, line)
        | Stmt::If { line, .. }
        | Stmt::For { line, .. }
        | Stmt::ForRange { line, .. }
        | Stmt::Go { line, .. }
        | Stmt::Defer { line, .. }
        | Stmt::Send { line, .. }
        | Stmt::Select { line, .. }
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
        Stmt::IncDec { .. } | Stmt::Break(_) | Stmt::Continue(_) => false,
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
