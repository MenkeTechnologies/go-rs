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

/// The bit width and signedness of a Go integer type narrower than 64 bits —
/// the ones whose arithmetic wraps somewhere other than where `Value::Int`
/// (an `i64`) already wraps. `int` / `int64` / `uint64` / `uintptr` are 64-bit,
/// so 64-bit two's-complement wrapping is already Go's answer for them.
fn int_width(ty: &str) -> Option<(u32, bool)> {
    Some(match ty {
        "int8" => (8, true),
        "int16" => (16, true),
        "int32" | "rune" => (32, true),
        "uint8" | "byte" => (8, false),
        "uint16" => (16, false),
        "uint32" => (32, false),
        _ => return None,
    })
}

/// Whether `for … range x` over a value of static type `ty` walks the index
/// sequence `0 … n-1`.
///
/// A slice, a fixed-size array and — since Go 1.22 — an integer all do. A map
/// does not: its keys are its own values. Neither does a string: `range` walks
/// it by rune, so the keys are the byte offsets each rune *starts* at, which
/// skip the continuation bytes of every multi-byte one.
///
/// An empty `ty` is an unknown static type rather than a claim about the
/// value, so it answers `false` and takes the general path.
fn integer_keyed_range(ty: &str) -> bool {
    ty.starts_with("[]") || array_elem_ty(ty).is_some() || int_range_ty(ty)
}

/// Whether `ty` is one of the integer types Go 1.22's `for i := range n`
/// accepts, whose iteration count is the value itself rather than a length.
fn int_range_ty(ty: &str) -> bool {
    matches!(ty, "int" | "int64" | "uint" | "uint64" | "uintptr") || int_width(ty).is_some()
}

/// Whether `ty` is one of Go's unsigned 64-bit integer types. They are the
/// widths `Value::Int` (an `i64`) holds bit-identically but *reads* differently:
/// every operation that consults the sign bit needs an unsigned form.
fn is_uint64_ty(ty: &str) -> bool {
    matches!(ty, "uint64" | "uint" | "uintptr")
}

/// Whether `ty` is a sized integer type that `Value::Int` represents but `%T`
/// must not call `int`.
///
/// Every one of these is an `i64` at run time — [`int_width`] already makes the
/// *arithmetic* wrap at the right bit — so the width survives in the static type
/// alone. `%T` is the one verb that reads it, which is why the name is attached
/// only at a `fmt` argument position (see `Compiler::sized_int_box_spec`) and
/// never enters the value flow.
///
/// `int` is excluded because `%T` already prints it, and the unsigned 64-bit
/// types are excluded because [`is_uint64_ty`] tags them through `GU64_BOX`,
/// which carries their *signedness* as well as their name.
fn is_sized_int_ty(ty: &str) -> bool {
    matches!(
        ty,
        "int8" | "int16" | "int32" | "int64" | "uint8" | "uint16" | "uint32" | "byte" | "rune"
    )
}

/// The [`host::u64_op`] code for an operator whose result depends on the sign
/// bit, or `None` for the ones two's complement already makes signedness-blind
/// (`+ - * << & | ^ &^`) and for the operators that are not arithmetic at all.
fn u64_op_code(op: BinOp) -> Option<i64> {
    Some(match op {
        BinOp::Div => host::u64_op::DIV,
        BinOp::Mod => host::u64_op::MOD,
        BinOp::Shr => host::u64_op::SHR,
        BinOp::Lt => host::u64_op::LT,
        BinOp::Le => host::u64_op::LE,
        BinOp::Gt => host::u64_op::GT,
        BinOp::Ge => host::u64_op::GE,
        _ => return None,
    })
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

/// The value type written in a `map[K]V`, exactly as written — `*T` keeps its
/// star, which is what tells a nil pointer apart from a zero struct. `None` for
/// anything that is not a map type.
///
/// The key can carry brackets of its own (`map[[2]int]V`) and so can the value
/// (`map[string][]T`), so the key ends at the `]` that closes the one `map[`
/// opened — found by depth, not by the first or last `]`.
fn map_value_ty(ty: &str) -> Option<&str> {
    map_split(ty).map(|(_, v)| v)
}

/// The key type written in a `map[K]V`, exactly as written. `None` for anything
/// that is not a map type.
fn map_key_ty(ty: &str) -> Option<&str> {
    map_split(ty).map(|(k, _)| k)
}

/// Split `map[K]V` into `(K, V)` at the `]` that closes the one `map[` opened.
fn map_split(ty: &str) -> Option<(&str, &str)> {
    let rest = ty.strip_prefix("map[")?;
    let mut depth = 0usize;
    for (i, ch) in rest.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' if depth == 0 => return Some((&rest[..i], &rest[i + 1..])),
            ']' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// The element type named by a container type — `[]T` and `map[K]V` both yield
/// `T`/`V`. `None` for anything that is not a container.
fn elem_of_type(ty: &str) -> Option<String> {
    if let Some(t) = array_elem_ty(ty) {
        return Some(t.to_string());
    }
    if let Some(t) = ty.strip_prefix("[]") {
        return Some(t.to_string());
    }
    ty.strip_prefix("map[")
        .and_then(|t| t.split_once(']'))
        .map(|(_, v)| v.to_string())
}

/// The [`host::f32_op`] code for an arithmetic operator, or `None` for the ones
/// Go does not define on floats (`%`, the bitwise set).
fn f32_op_code(op: BinOp) -> Option<i64> {
    Some(match op {
        BinOp::Add => host::f32_op::ADD,
        BinOp::Sub => host::f32_op::SUB,
        BinOp::Mul => host::f32_op::MUL,
        BinOp::Div => host::f32_op::DIV,
        _ => return None,
    })
}

/// A top-level function's signature, for call resolution and return typing.
struct FuncSig {
    arity: usize,
    /// Each parameter's declared Go type, in order. Go converts an untyped
    /// constant argument to the parameter's type at the call — `f(1)` on a
    /// `func f(x float64)` passes a `float64` — and nothing downstream can tell
    /// the two apart once the value has been pushed, so the call site is where
    /// the conversion has to happen.
    param_tys: Vec<String>,
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
    /// Aligned with `captures`: each one's declared Go type as the *enclosing*
    /// scope knew it. A lambda body is compiled with a fresh symbol table, so
    /// without this a captured `chan int` or `float32` would be untyped inside
    /// the closure and lower as if it were an untyped value — which silently
    /// turned `for j := range jobs` on a captured channel into a range over the
    /// channel handle's integer id.
    capture_types: Vec<String>,
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

/// The two temporaries [`Compiler::comma_ok`] leaves a comma-ok form in, plus
/// the value's inferred and written types — what a `:=` needs to declare its
/// name at, and what a `=` reads back to coerce into an existing one.
struct CommaOk {
    value: String,
    ok: String,
    value_num_ty: NumType,
    value_decl_ty: String,
}

/// Back-patch targets for one enclosing breakable construct (`for` loop or
/// `switch`). `break` targets the innermost; `continue` targets the innermost
/// non-switch (loop), so a `continue` inside a switch reaches the enclosing loop.
#[derive(Default)]
struct LoopScope {
    /// The `label:` written immediately before this loop, if any. A labeled
    /// `break`/`continue` names it to leave or step an *outer* loop rather than
    /// the innermost one.
    label: Option<String>,
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
    /// `(type, method)` for every method declared with a value receiver — the
    /// receiver is copied at the call, so the method's writes stay local.
    value_recv_methods: HashSet<(String, String)>,
    /// Method result counts keyed by `(receiver type, method name)` — lets a
    /// `v, ok := recv.M()` destructure a multi-value method return.
    method_nresults: HashMap<(String, String), usize>,
    /// Each method's declared result type, keyed by `(receiver type, method name)`.
    /// It is the only static-type path to a method call's result, which `%T`
    /// needs when the method returns a defined type.
    method_result_ty: HashMap<(String, String), String>,
    /// Each method's declared parameter types, keyed by `(receiver type,
    /// method name)`. Go converts an untyped constant argument to the
    /// parameter's type at the call, the same as for a plain function.
    method_param_tys: HashMap<(String, String), Vec<String>>,
    /// Each interface type's method set, keyed by its name — both declared
    /// (`type Stringer interface{…}`) and anonymous (`interface{ Unwrap() error }`,
    /// registered by the parser under a canonical name). Only method-bearing
    /// interfaces are here; the empty interface matches everything and needs no
    /// test. A type assertion or type-switch case naming one of these lowers to a
    /// method-set check instead of a type-tag comparison.
    iface_methods: HashMap<String, Vec<String>>,
    /// Every declared interface type's name, including the method-less ones that
    /// `iface_methods` omits. Naming one in call position — `error(e)`, `any(3)`,
    /// `Stringer(v)` — is Go's identity conversion to an interface type.
    iface_names: HashSet<String>,
    /// `type Name <base>` over a non-struct, non-interface base, as name → base.
    /// Naming one in call position is a conversion, and the name is what `%T`
    /// prints — `main.Weekday`, not `int`.
    defined_types: HashMap<String, String>,
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
    /// The current function's declared result types, so `return nil` for a
    /// slice/map result emits that type's typed nil rather than a bare `Undef`.
    fn_results: Vec<String>,
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
            Stmt::Break(..) | Stmt::Continue(..) | Stmt::Fallthrough(_) => false,
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
            Expr::MakeChan { cap: Some(c), .. } => ex(c, out),
            Expr::MakeChan { cap: None, .. } => {}
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
                if let SelectComm::Recv { bind, ok_bind, .. } = &c.comm {
                    for b in [bind, ok_bind].into_iter().flatten() {
                        out.insert(b.clone());
                    }
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
                    SelectComm::Recv {
                        bind,
                        ok_bind,
                        chan,
                    } => {
                        fe(chan, bound, out);
                        for b in [bind, ok_bind].into_iter().flatten() {
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
        Stmt::Break(..) | Stmt::Continue(..) | Stmt::Fallthrough(_) => {}
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
        Expr::MakeChan { cap: Some(c), .. } => free_expr(c, bound, out),
        Expr::MakeChan { cap: None, .. } => {}
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
        Stmt::Break(..) | Stmt::Continue(..) | Stmt::Fallthrough(_) => {}
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

/// Synthesize the forwarding methods Go's field/method promotion implies: for
/// every struct that embeds another, each method of the embedded type that the
/// outer type does not declare itself becomes a real method on the outer type
/// whose body forwards to the embedded value. Making them real methods (rather
/// than a lookup rule) means static dispatch, dynamic dispatch, and interface
/// satisfaction all see the promoted method with no further special-casing.
fn promoted_methods(prog: &Program) -> Vec<Func> {
    let field_types: HashMap<&str, Vec<&Param>> = prog
        .types
        .iter()
        .map(|t| (t.name.as_str(), t.fields.iter().collect()))
        .collect();
    // A field is embedded when its name is its type's own name.
    let embedded = |ty: &str| -> Vec<String> {
        field_types
            .get(ty)
            .map(|fs| {
                fs.iter()
                    .filter(|p| p.name == base_type(&p.ty).rsplit('.').next().unwrap_or_default())
                    .map(|p| p.name.clone())
                    .collect()
            })
            .unwrap_or_default()
    };
    let declared: HashSet<(&str, &str)> = prog
        .funcs
        .iter()
        .filter_map(|f| {
            f.receiver
                .as_ref()
                .map(|r| (base_type(&r.ty), f.name.as_str()))
        })
        .map(|(t, m)| {
            (
                field_types.keys().find(|k| **k == t).copied().unwrap_or(""),
                m,
            )
        })
        .filter(|(t, _)| !t.is_empty())
        .collect();

    let mut out: Vec<Func> = Vec::new();
    // Each round promotes one level of embedding, so a method reaches an outer
    // type through a chain of embedded fields as well as a single one.
    for _ in 0..8 {
        let mut round: Vec<Func> = Vec::new();
        // Everything visible on a type so far: its own methods plus the
        // forwarders already synthesized for it.
        let have = |ty: &str, m: &str, out: &[Func]| -> bool {
            declared.contains(&(ty, m))
                || out.iter().any(|f| {
                    f.name == m && f.receiver.as_ref().is_some_and(|r| base_type(&r.ty) == ty)
                })
        };
        for t in prog.types.iter() {
            for inner in embedded(&t.name) {
                let sources: Vec<&Func> = prog
                    .funcs
                    .iter()
                    .filter(|f| {
                        f.receiver
                            .as_ref()
                            .is_some_and(|r| base_type(&r.ty) == inner)
                    })
                    .chain(out.iter().filter(|f| {
                        f.receiver
                            .as_ref()
                            .is_some_and(|r| base_type(&r.ty) == inner)
                    }))
                    .collect();
                for src in sources {
                    if have(&t.name, &src.name, &out) || have(&t.name, &src.name, &round) {
                        continue; // the outer type overrides it
                    }
                    round.push(forwarder(&t.name, &inner, src));
                }
            }
        }
        if round.is_empty() {
            break;
        }
        out.extend(round);
    }
    out
}

/// `func (r Outer) m(args…) … { [return] r.Inner.m(args…) }` — the body of one
/// promoted method.
fn forwarder(outer: &str, inner: &str, src: &Func) -> Func {
    let recv = "$r".to_string();
    let args: Vec<Expr> = src
        .params
        .iter()
        .map(|p| Expr::Ident(p.name.clone()))
        .collect();
    let call = Expr::Call {
        func: Box::new(Expr::Selector {
            recv: Box::new(Expr::Selector {
                recv: Box::new(Expr::Ident(recv.clone())),
                field: inner.to_string(),
            }),
            field: src.name.clone(),
        }),
        args,
        spread: src.variadic,
        line: src.line,
    };
    let body = if src.results.is_empty() {
        vec![Stmt::ExprStmt(call)]
    } else {
        vec![Stmt::Return(vec![call], src.line)]
    };
    // The forwarder inherits the promoted method's receiver kind. Go promotes a
    // pointer-receiver method into the outer type's *pointer* method set, so
    // `d.Bump()` on an addressable `d` mutates the embedded field; giving the
    // forwarder a value receiver would copy `d` and drop the write.
    let by_pointer = src.receiver.as_ref().is_some_and(|r| r.ty.starts_with('*'));
    Func {
        name: src.name.clone(),
        receiver: Some(Param {
            name: recv,
            ty: if by_pointer {
                format!("*{outer}")
            } else {
                outer.to_string()
            },
        }),
        params: src.params.clone(),
        variadic: src.variadic,
        results: src.results.clone(),
        result_names: vec![String::new(); src.results.len()],
        body,
        line: src.line,
    }
}

fn compile_with(prog: &Program, debug: bool) -> Result<Chunk, String> {
    // Promotion is a source-level rewrite: the forwarders join the program's
    // own functions before anything else looks at the method set.
    let promoted = promoted_methods(prog);
    let prog = &if promoted.is_empty() {
        prog.clone()
    } else {
        let mut p = prog.clone();
        p.funcs.extend(promoted);
        p
    };
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
    let mut method_result_ty: HashMap<(String, String), String> = HashMap::new();
    let mut method_param_tys: HashMap<(String, String), Vec<String>> = HashMap::new();
    // Methods declared with a *value* receiver (`func (t T)`, not `func (t *T)`).
    // Go binds such a receiver to a copy, so the method cannot mutate the caller's
    // struct; a pointer receiver binds the struct itself and is meant to.
    let mut value_recv_methods: HashSet<(String, String)> = HashSet::new();
    for f in &prog.funcs {
        match &f.receiver {
            Some(r) => {
                methods.insert((base_type(&r.ty), f.name.clone()), f.params.len());
                method_nresults.insert((base_type(&r.ty), f.name.clone()), f.results.len());
                method_param_tys.insert(
                    (base_type(&r.ty), f.name.clone()),
                    f.params.iter().map(|p| p.ty.clone()).collect(),
                );
                if let [result] = f.results.as_slice() {
                    method_result_ty.insert((base_type(&r.ty), f.name.clone()), result.clone());
                }
                if !r.ty.starts_with('*') {
                    value_recv_methods.insert((base_type(&r.ty), f.name.clone()));
                }
            }
            None => {
                funcs.insert(
                    f.name.clone(),
                    FuncSig {
                        arity: f.params.len(),
                        param_tys: f.params.iter().map(|p| p.ty.clone()).collect(),
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

    // Each concrete type's method set, and each method-bearing interface's — the
    // two halves of Go's interface satisfaction rule, resolved at run time
    // against the table the prologue registers.
    let mut type_methods: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for f in &prog.funcs {
        if let Some(r) = &f.receiver {
            type_methods
                .entry(base_type(&r.ty))
                .or_default()
                .push(method_sig(&f.name, f.params.len(), &f.results));
        }
    }
    // Go promotes an embedded type's methods into the embedding struct's method
    // set, so a struct satisfies an interface its embedded field implements.
    for _ in 0..prog.types.len() {
        let mut grew = false;
        for t in &prog.types {
            // An embedded field is the one whose name is its own type.
            let promoted: Vec<String> = t
                .fields
                .iter()
                .filter(|f| f.name == base_type(&f.ty).rsplit('.').next().unwrap_or_default())
                .filter_map(|f| type_methods.get(&base_type(&f.ty)).cloned())
                .flatten()
                .collect();
            let own = type_methods.entry(t.name.clone()).or_default();
            for m in promoted {
                if !own.contains(&m) {
                    own.push(m);
                    grew = true;
                }
            }
        }
        if !grew {
            break;
        }
    }
    for ms in type_methods.values_mut() {
        ms.sort();
        ms.dedup();
    }
    let mut iface_methods: HashMap<String, Vec<String>> = prog
        .interfaces
        .iter()
        .filter(|i| !i.methods.is_empty())
        .map(|i| {
            let mut ms = i.methods.clone();
            ms.sort();
            ms.dedup();
            (i.name.clone(), ms)
        })
        .collect();
    // `error` is predeclared, so it never appears in `prog.interfaces` — but it
    // is `interface{ Error() string }`, not the empty interface. Leaving it out
    // made `type_to_tag` answer the empty tag, which `emit_type_test` reads as
    // "matches everything": `case error:` then took a `float64`.
    iface_methods
        .entry("error".to_string())
        .or_insert_with(|| vec![method_sig("Error", 0, &["string".to_string()])]);

    // Every interface name usable as an identity conversion, plus the two
    // predeclared ones (`error` and `any`/`interface{}`) a program never declares.
    let mut iface_names: HashSet<String> = prog.interfaces.iter().map(|i| i.name.clone()).collect();
    iface_names.insert("error".into());
    iface_names.insert("any".into());

    let has_ffi = body_has_ffi(&prog.main) || prog.funcs.iter().any(|f| body_has_ffi(&f.body));
    // Package-level names: variables/constants declared at the top level of
    // `main` (which, after linking, holds every package's init-order globals
    // ahead of `main`'s own body). Functions read these as globals.
    let globals = collect_globals(&prog.main);

    // Tell the runtime which fields a struct value-copy must recurse into: those
    // declared as a struct type by value. A `*T` field keeps its raw `*` here, so
    // it is not in the plan and stays aliased — Go copies a pointer, not what it
    // points at.
    // A fixed-size array field is a value too, and the plan carries its written
    // type so the copy knows whether the array's own elements are values.
    host::set_struct_plan(
        struct_fields
            .iter()
            .map(|(name, fields)| {
                (
                    name.clone(),
                    fields
                        .iter()
                        .filter(|(_, ty)| {
                            structs.contains(ty.as_str()) || array_elem_ty(ty).is_some()
                        })
                        .map(|(f, ty)| (f.clone(), ty.clone()))
                        .collect(),
                )
            })
            .collect(),
    );

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
        value_recv_methods,
        method_nresults,
        method_result_ty,
        method_param_tys,
        iface_methods,
        iface_names,
        defined_types: prog.defined.iter().cloned().collect(),
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
        fn_results: Vec::new(),
    };

    // ── main body (global scope) ──
    // A program that uses panic/recover routes runtime faults (divide-by-zero,
    // index-out-of-range, nil dereference) through the recoverable panic path.
    if c.uses_panic {
        c.b.emit(Op::CallBuiltin(host::GSET_PANIC_MODE, 0), 0);
        c.b.emit(Op::Pop, 0);
    }
    // Publish every concrete type's method set, which is the half of Go's
    // interface satisfaction rule the run time has to look up. A program with no
    // methods at all registers nothing; one with methods pays a constant load
    // and a call per type, once, in the prologue. (It cannot be gated on the
    // program naming an interface: `error` is predeclared, so every program can
    // test against one without declaring anything.)
    if !type_methods.is_empty() {
        for (ty, ms) in &type_methods {
            let t = c.b.add_constant(Value::str(ty.clone()));
            c.b.emit(Op::LoadConst(t), 0);
            let m = c.b.add_constant(Value::str(ms.join(",")));
            c.b.emit(Op::LoadConst(m), 0);
            c.b.emit(Op::CallBuiltin(host::GREG_METHODS, 2), 0);
            c.b.emit(Op::Pop, 0);
        }
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
        for (n, p) in f.params.iter().enumerate() {
            scope.slots.insert(p.name.clone(), slot);
            // `Param::ty` of a variadic parameter is its *element* type — the
            // call site needs that to build the trailing slice — but inside the
            // body the name is bound to the slice, so that is the type recorded
            // here. Without the distinction `xs ...int` reads as a plain `int`,
            // and anything deciding by static type treats the slice handle as
            // the number it is stored as.
            let ty = if f.variadic && n + 1 == f.params.len() {
                format!("[]{}", base_type(&p.ty))
            } else {
                base_type(&p.ty)
            };
            self.types.insert(p.name.clone(), numtype_of_ty(&ty));
            self.decl_types.insert(p.name.clone(), ty);
            slot += 1;
        }
        scope.next_slot = slot;
        self.scope = Some(scope);

        // Named results become zero-initialized locals the body may read/assign.
        self.fn_results = f.results.clone();
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
                self.emit_zero(ty, f.line);
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
        self.fn_results = Vec::new();
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
        let capture_types = captures
            .iter()
            .map(|c| self.decl_types.get(c).cloned().unwrap_or_default())
            .collect();
        self.lambdas.push(LambdaInfo {
            params: params.to_vec(),
            body: body.to_vec(),
            captures,
            cell_captures,
            capture_types,
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
            let _ = self.emit_lit_chunked(host::GSLICE_LIT, 0, 1, names.len(), line, |c, i| {
                c.emit_get(&names[i], line);
                Ok(())
            });
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
            let results = results.to_vec();
            let _ = self.emit_lit_chunked(host::GSLICE_LIT, 0, 1, results.len(), line, |c, i| {
                c.emit_zero(&results[i], line);
                Ok(())
            });
        } else if let Some(ty) = results.first() {
            self.emit_zero(ty, line);
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
        // Park any propagating panic across the call: a deferred function runs
        // normally in Go, so the post-call unwind checks inside it must not see
        // the panic it was deferred for. Park records this frame's depth, which
        // is what lets `recover()` tell its *direct* deferred caller from a
        // helper that deferred function called in turn.
        self.b.emit(Op::CallBuiltin(host::GDEFER_PARK, 0), 0);
        self.b.emit(Op::Pop, 0);
        self.emit_get("$dcpop", 0); // the closure, as its own "self"
        self.emit_get("$dcpop", 0);
        self.b.emit(Op::CallBuiltin(host::GCLOSURE_NAMEIDX, 1), 0);
        self.b.emit(Op::CallDynamic(1), 0);
        self.b.emit(Op::Pop, 0); // discard the deferred call's result
        self.b.emit(Op::CallBuiltin(host::GDEFER_UNPARK, 0), 0);
        self.b.emit(Op::Pop, 0);
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
        // The lambda's own parameter types are known here, so an untyped
        // constant argument converts the way it does for a named function.
        let param_tys: Vec<String> = self
            .lambdas
            .get(id as usize)
            .map(|l| l.params.iter().map(|p| p.ty.clone()).collect())
            .unwrap_or_default();
        for (i, a) in args.iter().enumerate() {
            self.emit_arg(a, param_tys.get(i))?;
        }
        let idx = self.b.add_name(&format!("$lambda_{id}"));
        self.b.emit(Op::Call(idx, args.len() as u8 + 1), line);
        self.emit_panic_check(line);
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
        let capture_types = self.lambdas[id].capture_types.clone();

        let entry = self.b.current_pos();
        let name_idx = self.b.add_name(&format!("$lambda_{id}"));
        self.b.add_sub_entry(name_idx, entry);

        let mut scope = Scope::new();
        self.types.clear();
        self.decl_types.clear();
        // Re-seed the captured names with the types the enclosing scope had, so
        // a captured channel, `float32` or `uint64` keeps lowering by its type.
        for (name, ty) in captures.iter().zip(&capture_types) {
            if !ty.is_empty() {
                self.types.insert(name.clone(), numtype_of_ty(ty));
                self.decl_types.insert(name.clone(), ty.clone());
            }
        }
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
                        SelectComm::Recv {
                            bind,
                            ok_bind,
                            chan,
                        } => {
                            self.fv_expr(chan, bound, caps);
                            for v in [bind, ok_bind].into_iter().flatten() {
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
            Stmt::Break(..) | Stmt::Continue(..) | Stmt::Fallthrough(_) => {}
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
            Expr::Slice {
                recv,
                low,
                high,
                max,
            } => {
                self.fv_expr(recv, bound, caps);
                for e in [low, high, max].into_iter().flatten() {
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
            Expr::MakeChan { cap, .. } => {
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
                    (Some(t), _) => numtype_of_ty(&self.underlying(&base_type(t))),
                    (None, Some(e)) => self.infer(e),
                    (None, None) => NumType::Unknown,
                };
                let decl_ty = match (ty, init) {
                    (Some(t), _) => base_type(t),
                    (None, Some(e)) => self.type_name(e),
                    (None, None) => String::new(),
                };
                if let Some(k) = map_key_ty(&decl_ty) {
                    self.check_map_key(k, *line)?;
                }
                // `var s T` where T is a *value* struct type → its zero value is a
                // struct with every field zeroed (so `s.f` and methods work). A
                // pointer `var p *T` is nil, not a zero struct.
                let is_pointer = ty.as_ref().is_some_and(|t| t.starts_with('*'));
                match init {
                    // `var x float64 = 3` stores a float, not the raw integer
                    // constant — Go converts on assignment to a declared type.
                    Some(e)
                        if (nt == NumType::Float && self.infer(e) == NumType::Int)
                            || decl_ty == "float32" =>
                    {
                        let t = ty.clone().unwrap_or_default();
                        self.closure_vars.remove(name);
                        self.emit_typed(e, &t)?;
                    }
                    // `var s []int = nil` — the written type makes the `nil` a
                    // typed one, so it prints `[]` like the initializer-less form.
                    Some(e) if self.is_nil_literal(e) => {
                        let t = ty.clone().unwrap_or_default();
                        self.closure_vars.remove(name);
                        self.emit_zero(&t, *line);
                    }
                    Some(e) => self.emit_rhs(name, e)?,
                    None if !is_pointer && self.structs.contains(&decl_ty) => {
                        self.struct_lit(&decl_ty, &[])?
                    }
                    // `var s []int` / `var m map[string]int` — the written type
                    // chooses the typed nil; without one there is nothing to type.
                    None => match ty {
                        Some(t) => self.emit_zero(t, *line),
                        None => self.emit_default(nt, *line),
                    },
                }
                // `var a [N]T` is an array slot however it was initialized, so
                // it stamps the written type for `%T`/`%#v`. The form without an
                // initializer lowers to a `make`, which carries no type of its
                // own; the other forms re-stamp the type they already have.
                if let Some(t) = ty.as_deref().filter(|t| array_elem_ty(t).is_some()) {
                    let t = t.to_string();
                    self.emit_array_tag(&t, *line);
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
                // `v, ok := x.(T)` / `<-ch` / `m[k]` — the three comma-ok forms,
                // lowered to a pair of temporaries and declared from them.
                if names.len() == 2 && values.len() == 1 {
                    if let Some(co) = self.comma_ok(&values[0], *line)? {
                        self.emit_get(&co.value, *line);
                        self.types.insert(names[0].clone(), co.value_num_ty);
                        self.decl_types
                            .insert(names[0].clone(), co.value_decl_ty.clone());
                        self.emit_declare(&names[0], *line);
                        self.emit_get(&co.ok, *line);
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
                        // `n := strconv.Atoi(s)` — Go rejects binding a two-value
                        // call to one name.
                        self.check_single_value(e, *line)?;
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
                        let tys = self.fn_results.clone();
                        // `return f()` forwarding a call that yields exactly as
                        // many values as there are results — Go assigns them
                        // position by position. Zipping the names against the
                        // one written value instead puts the callee's whole
                        // tuple in the first name and leaves the rest at their
                        // zero, which is what made `io.WriteString` answer
                        // `[2 <nil>] <nil>`.
                        if vals.len() == 1
                            && names.len() >= 2
                            && self.call_result_count(&vals[0]) == Some(names.len())
                        {
                            let n = self.temp_counter;
                            self.temp_counter += 1;
                            let tup = format!("$rt{n}");
                            self.expr(&vals[0])?;
                            self.types.insert(tup.clone(), NumType::Unknown);
                            self.emit_set(&tup, *line);
                            for (i, name) in names.iter().enumerate() {
                                self.emit_get(&tup, *line);
                                self.b.emit(Op::LoadInt(i as i64), *line);
                                self.b.emit(Op::CallBuiltin(host::GINDEX_GET, 2), *line);
                                self.emit_set(name, *line);
                            }
                        } else {
                            for (i, (name, e)) in names.iter().zip(vals).enumerate() {
                                self.emit_result(e, i, &tys)?;
                                self.emit_set(name, *line);
                            }
                        }
                    }
                    self.emit_named_return(*line);
                }
                Some(_) => {
                    match vals.len() {
                        0 => {
                            self.b.emit(Op::LoadUndef, *line);
                        }
                        1 => {
                            let tys = self.fn_results.clone();
                            self.emit_result(&vals[0], 0, &tys)?
                        }
                        // Multiple results are returned as one tuple (a slice
                        // heap value), destructured at the call site.
                        n => {
                            let tys = self.fn_results.clone();
                            let vals = vals.clone();
                            self.emit_lit_chunked(host::GSLICE_LIT, 0, 1, n, *line, |c, i| {
                                c.emit_result(&vals[i], i, &tys)
                            })?;
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
                label,
                ..
            } => self.compile_for(init, cond, post, body, label)?,
            Stmt::ForRange {
                key,
                val,
                iter,
                body,
                label,
                ..
            } => self.compile_for_range(key, val, iter, body, label)?,
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
                // A send transfers a *copy* of a struct value: the sender may go
                // on mutating its own variable without the receiver seeing it.
                // It also converts an untyped constant to the element type, so
                // `ch <- 1` on a `chan float64` sends a float.
                let elem = self.chan_elem_ty(chan);
                self.emit_typed(val, &elem)?;
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
                label,
            } => self.compile_switch(init, tag, cases, default, *line, label)?,
            Stmt::TypeSwitch {
                init,
                bind,
                expr,
                cases,
                default,
                line,
            } => self.compile_type_switch(init, bind, expr, cases, default, *line)?,
            Stmt::Break(line, label) => {
                let j = self.b.emit(Op::Jump(0), *line);
                // A labeled `break` leaves the loop or `switch` carrying that
                // label, however many scopes out it is; an unlabeled one leaves
                // the innermost.
                let scope = match label {
                    Some(l) => self
                        .loops
                        .iter_mut()
                        .rev()
                        .find(|s| s.label.as_deref() == Some(l.as_str()))
                        .ok_or_else(|| {
                            format!("go-rs: no enclosing label `{l}` for `break` (line {line})")
                        })?,
                    None => self
                        .loops
                        .last_mut()
                        .ok_or_else(|| format!("go-rs: `break` outside a loop (line {line})"))?,
                };
                scope.breaks.push(j);
            }
            Stmt::Continue(line, label) => {
                let j = self.b.emit(Op::Jump(0), *line);
                // `continue` targets a *loop*, so a `switch` scope is skipped
                // whether or not it carries the named label — Go only allows the
                // label of an enclosing loop here.
                let scope = match label {
                    Some(l) => self
                        .loops
                        .iter_mut()
                        .rev()
                        .find(|s| !s.is_switch && s.label.as_deref() == Some(l.as_str()))
                        .ok_or_else(|| {
                            format!("go-rs: no enclosing loop labeled `{l}` for `continue` (line {line})")
                        })?,
                    None => self
                        .loops
                        .iter_mut()
                        .rev()
                        .find(|s| !s.is_switch)
                        .ok_or_else(|| format!("go-rs: `continue` outside a loop (line {line})"))?,
                };
                scope.continues.push(j);
            }
            Stmt::Block(stmts) => {
                for s in stmts {
                    self.stmt(s)?;
                }
            }
        }
        Ok(())
    }

    /// Lower `for init; cond; post { … }` — which is also Go's `while` (`for
    /// cond {}`) and its infinite loop (`for {}`) — **rotated**: the condition
    /// is emitted once above the body as an entry guard and once below it as a
    /// *conditional backward branch*, instead of once at the top closed by an
    /// unconditional `Jump` back to it.
    ///
    /// The shape is what fusevm's tracing JIT requires: it only closes a
    /// recorded trace on a conditional backward branch. Emitted the other way
    /// `go --tiers` reported `trace-eligible=true traced=false` and `reaches
    /// native code false` for every counted loop go-rs produced, so the hottest
    /// shape a Go program has stayed in the interpreter however hot it got.
    ///
    /// Evaluation order and count are unchanged: a top-test loop evaluates the
    /// condition `n + 1` times for `n` iterations, and so does this — once on
    /// entry, then once after each body-and-post run. Rotation costs one copy
    /// of the condition's code and saves one jump per iteration.
    ///
    /// `for {}` has no condition, so it needs no entry guard and branches back
    /// on a constant `true`: one push and pop per iteration buys the same
    /// compilable shape, which an `Op::Jump` back edge does not get.
    fn compile_for(
        &mut self,
        init: &Option<Box<Stmt>>,
        cond: &Option<Expr>,
        post: &Option<Box<Stmt>>,
        body: &[Stmt],
        label: &Option<String>,
    ) -> Result<(), String> {
        if let Some(init) = init {
            self.stmt(init)?;
        }
        self.loops.push(LoopScope {
            label: label.clone(),
            ..Default::default()
        });
        // The entry guard. It leaves the loop when the condition is false on
        // arrival, which is the only thing the top copy still does.
        let guard = match cond {
            Some(c) => {
                self.expr(c)?;
                Some(self.b.emit(Op::JumpIfFalse(0), 0))
            }
            None => None,
        };
        let top = self.b.current_pos();
        for s in body {
            self.stmt(s)?;
        }
        // `continue` lands here — run the post statement, then re-test.
        let post_pos = self.b.current_pos();
        if let Some(p) = post {
            self.stmt(p)?;
        }
        // The back edge, always conditional. `for {}` has no condition, so it
        // branches on a constant `true` — one extra push and pop per iteration,
        // in exchange for a shape the tracing JIT will compile. An `Op::Jump`
        // here is what fusevm's recorder declines: the same `for {}` measured
        // with an unconditional back edge reports `trace-eligible=true
        // traced=false`, and with this one `traced=true`.
        match cond {
            Some(c) => self.expr(c)?,
            None => {
                self.b.emit(Op::LoadTrue, 0);
            }
        }
        self.b.emit(Op::JumpIfTrue(top), 0);
        let end = self.b.current_pos();

        let scope = self.loops.pop().unwrap();
        if let Some(jf) = guard {
            self.b.patch_jump(jf, end);
        }
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
        label: &Option<String>,
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
            label: label.clone(),
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
                if !self.emit_type_test(&val, &tag, ty, line) {
                    // The empty interface (`any`) matches unconditionally.
                    match_jumps.push(self.b.emit(Op::Jump(0), line));
                    break;
                }
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
            // Bind the value to `v` inside the body. Go gives the binding the
            // case's own type when the case names exactly one — which is what
            // makes `v.Error()` legal under `case error:` in a program that
            // declares no error type of its own — and the switch expression's
            // type when it names several.
            if let Some(name) = bind {
                self.emit_get(&val, line);
                self.types.insert(name.clone(), NumType::Unknown);
                let bound_ty = match case.types.as_slice() {
                    [only] => only.clone(),
                    _ => String::new(),
                };
                self.decl_types.insert(name.clone(), bound_ty);
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

    /// The method set of `ty` when it names a method-bearing interface — the
    /// types an assertion against it must accept.
    fn iface_of(&self, ty: &str) -> Option<&Vec<String>> {
        self.iface_methods.get(&base_type(ty))
    }

    /// Emit a test of whether the value in `val_tmp` (whose runtime type tag is
    /// in `tag_tmp`) has type `ty`, leaving a bool on the stack. Returns `false`
    /// — emitting nothing — when `ty` is the empty interface, which every value
    /// satisfies.
    ///
    /// An interface with a method set is satisfied by any type implementing every
    /// method, so it tests the method set rather than the type tag; that is what
    /// `errors.Is`/`errors.As` use (`err.(interface{ Unwrap() error })`) to walk
    /// a wrap chain without naming any concrete error type.
    fn emit_type_test(&mut self, val_tmp: &str, tag_tmp: &str, ty: &str, line: u32) -> bool {
        if let Some(ms) = self.iface_of(ty) {
            let want = ms.join(",");
            self.emit_get(val_tmp, line);
            let c = self.b.add_constant(Value::str(want));
            self.b.emit(Op::LoadConst(c), line);
            self.b.emit(Op::CallBuiltin(host::GIFACE_OK, 2), line);
            return true;
        }
        let tag = type_to_tag(ty);
        if tag.is_empty() {
            return false;
        }
        self.emit_get(tag_tmp, line);
        let c = self.b.add_constant(Value::str(tag));
        self.b.emit(Op::LoadConst(c), line);
        self.b.emit(Op::StrEq, line);
        true
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
            if let SelectComm::Recv {
                bind,
                ok_bind,
                chan,
            } = &c.comm
            {
                let elem = self.chan_elem_ty(chan);
                // `case v, ok := <-ch:` — a closed channel makes its receive case
                // *ready*, so this is how a select loop learns a channel is
                // finished. `ok` is false exactly for that delivery.
                if let Some(o) = ok_bind {
                    self.emit_get(&sv, line);
                    self.b.emit(Op::CallBuiltin(host::GCHAN_OK, 1), line);
                    self.emit_set(o, line);
                    self.types.insert(o.clone(), NumType::Bool);
                }
                if let Some(v) = bind {
                    // The value a closed channel delivers is the element type's
                    // zero, so this binding needs the same sentinel mapping
                    // every other receive gets.
                    self.emit_get(&sv, line);
                    self.emit_elem_zero(&elem, line)?;
                    self.b.emit(Op::CallBuiltin(host::GCHAN_VAL, 2), line);
                    self.emit_set(v, line);
                    self.types.insert(v.clone(), NumType::Unknown);
                    self.decl_types.insert(v.clone(), elem);
                }
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

    /// Lower one of Go's three comma-ok expressions into a pair of temporaries.
    ///
    /// `x.(T)`, `<-ch` and `m[k]` each yield a value and a boolean when written
    /// with two names on the left, and each is legal with `:=` *and* with `=` —
    /// `v, ok = m[k]` into variables declared earlier is the idiom a loop body
    /// uses. Producing temporaries rather than binding names is what lets the
    /// declaring and the assigning statement share one lowering.
    ///
    /// `None` means `expr` is none of the three, and the caller falls through to
    /// its ordinary multi-value handling.
    fn comma_ok(&mut self, expr: &Expr, line: u32) -> Result<Option<CommaOk>, String> {
        let n = self.temp_counter;
        self.temp_counter += 1;
        match expr {
            // `ok` is whether the dynamic type matches; `v` is the value, or the
            // asserted type's zero when it does not (Go zeroes it).
            Expr::TypeAssert { expr, ty } => {
                let raw = format!("$ta{n}");
                let tag = format!("$tatag{n}");
                let ok = format!("$taok{n}");
                let val = format!("$tav{n}");
                self.expr(expr)?;
                self.types.insert(raw.clone(), NumType::Unknown);
                self.emit_set(&raw, line);
                self.emit_get(&raw, line);
                self.b.emit(Op::CallBuiltin(host::GTYPETAG, 1), line);
                self.types.insert(tag.clone(), NumType::Str);
                self.emit_set(&tag, line);
                // The empty interface, which every value satisfies, emits no
                // test at all and is always true.
                if !self.emit_type_test(&raw, &tag, ty, line) {
                    self.b.emit(Op::LoadTrue, line);
                }
                self.types.insert(ok.clone(), NumType::Bool);
                self.emit_set(&ok, line);

                self.emit_get(&ok, line);
                let to_zero = self.b.emit(Op::JumpIfFalse(0), line);
                self.emit_get(&raw, line);
                let done = self.b.emit(Op::Jump(0), line);
                let zpos = self.b.current_pos();
                self.b.patch_jump(to_zero, zpos);
                self.emit_zero(ty, line);
                let end = self.b.current_pos();
                self.b.patch_jump(done, end);
                let (num_ty, decl_ty) = (numtype_of_ty(ty), base_type(ty));
                self.types.insert(val.clone(), num_ty);
                self.decl_types.insert(val.clone(), decl_ty.clone());
                self.emit_set(&val, line);
                Ok(Some(CommaOk {
                    value: val,
                    ok,
                    value_num_ty: num_ty,
                    value_decl_ty: decl_ty,
                }))
            }
            // `ok` is false exactly when the channel was closed and drained,
            // which the scheduler signals with a sentinel; `v` is then the
            // element type's zero.
            Expr::Recv { chan } => {
                let raw = format!("$cr{n}");
                let ok = format!("$crok{n}");
                let val = format!("$crv{n}");
                let elem = self.chan_elem_ty(chan);
                self.expr(chan)?;
                self.b.emit(Op::ChanRecv, line);
                self.types.insert(raw.clone(), NumType::Unknown);
                self.emit_set(&raw, line);

                self.emit_get(&raw, line);
                self.b.emit(Op::CallBuiltin(host::GCHAN_OK, 1), line);
                self.types.insert(ok.clone(), NumType::Bool);
                self.emit_set(&ok, line);

                self.emit_get(&raw, line);
                self.emit_elem_zero(&elem, line)?;
                self.b.emit(Op::CallBuiltin(host::GCHAN_VAL, 2), line);
                let num_ty = numtype_of_ty(&elem);
                self.types.insert(val.clone(), num_ty);
                self.decl_types.insert(val.clone(), elem.clone());
                self.emit_set(&val, line);
                Ok(Some(CommaOk {
                    value: val,
                    ok,
                    value_num_ty: num_ty,
                    value_decl_ty: elem,
                }))
            }
            // `GMAP_GET2` yields a `[value, present]` pair to destructure.
            Expr::Index { recv, index } => {
                let pair = format!("$mg{n}");
                let ok = format!("$mgok{n}");
                let val = format!("$mgv{n}");
                self.expr(recv)?;
                self.emit_map_key(recv, index)?;
                // `v, ok := m[k]` zeroes `v` on a miss exactly as `m[k]` does,
                // so it takes the same value-type zero — and the type it names
                // is what the destructured `v` is then declared as, which keeps
                // `%T` and the width-sensitive arithmetic on it right.
                let argc = self.emit_map_miss_zero(recv, line)?;
                self.b.emit(Op::CallBuiltin(host::GMAP_GET2, argc), line);
                self.types.insert(pair.clone(), NumType::Unknown);
                self.emit_set(&pair, line);

                let v_ty = match argc {
                    3 => self.elem_type_of(&self.type_name(recv)),
                    _ => String::new(),
                };
                let v_num_ty = numtype_of_ty(&v_ty);
                self.emit_get(&pair, line);
                self.b.emit(Op::LoadInt(0), line);
                self.b.emit(Op::CallBuiltin(host::GINDEX_GET, 2), line);
                self.types.insert(val.clone(), v_num_ty);
                self.decl_types.insert(val.clone(), v_ty.clone());
                self.emit_set(&val, line);

                self.emit_get(&pair, line);
                self.b.emit(Op::LoadInt(1), line);
                self.b.emit(Op::CallBuiltin(host::GINDEX_GET, 2), line);
                self.types.insert(ok.clone(), NumType::Bool);
                self.emit_set(&ok, line);
                Ok(Some(CommaOk {
                    value: val,
                    ok,
                    value_num_ty: v_num_ty,
                    value_decl_ty: v_ty,
                }))
            }
            _ => Ok(None),
        }
    }

    /// Lower a parallel assignment `t… = v…`. Right-hand sides are evaluated into
    /// temporaries *first* (so `a, b = b, a` swaps), then each temp is assigned to
    /// its target. Also handles `a, b = f()` where a call returns exactly as many
    /// values as there are targets (destructuring the returned tuple).
    fn assign_multi(&mut self, targets: &[Expr], values: &[Expr], line: u32) -> Result<(), String> {
        let n = self.temp_counter;
        self.temp_counter += 1;

        // `v, ok = m[k]` / `x.(T)` / `<-ch` — the comma-ok forms assigning to
        // *existing* variables. Same lowering as the `:=` form, only assigning
        // rather than declaring.
        if targets.len() == 2 && values.len() == 1 {
            if let Some(co) = self.comma_ok(&values[0], line)? {
                self.assign(&targets[0], AssignOp::Set, &Expr::Ident(co.value), line)?;
                self.assign(&targets[1], AssignOp::Set, &Expr::Ident(co.ok), line)?;
                return Ok(());
            }
        }

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
                    let f32ish = self.is_f32(&Expr::Ident(name.clone())) || self.is_f32(value);
                    // `u /= n` / `u >>= n` read the sign bit; the target's own
                    // declared type decides, as it is the left operand.
                    let u64ish = self.is_u64(&Expr::Ident(name.clone()));
                    if !self.emit_f32_arith(assign_binop(op), f32ish, line)
                        && !self.emit_u64_arith(assign_binop(op), u64ish, line)
                    {
                        self.emit_arith(assign_binop(op), l, r, is_nonzero_const(value), line);
                        // `u8++` / `i8 += n` wrap at the variable's declared width.
                        if let Some(ty) = self.decl_types.get(name).cloned() {
                            self.emit_narrow(&ty, line);
                        }
                    }
                }
                self.emit_set(name, line);
            }
            Expr::Index { recv, index } => {
                self.expr(recv)?;
                self.emit_map_key(recv, index)?;
                if op == AssignOp::Set {
                    // `m[k] = v` / `xs[i] = v` stores a *copy* of a struct value,
                    // so a later write to `v` is not visible through the
                    // container — and converts an untyped constant to the
                    // element type, so `a[0] = 1` on a `[]float64` stores a
                    // float rather than an integer that only the static type
                    // says is one.
                    let elem = self.elem_type_of(&self.type_name(recv));
                    self.emit_typed(value, &elem)?;
                } else {
                    self.b.emit(Op::Dup2, line);
                    let argc = self.emit_map_miss_zero(recv, line)?;
                    self.b.emit(Op::CallBuiltin(host::GINDEX_GET, argc), line);
                    self.expr(value)?;
                    let f32ish = self.is_f32(target) || self.is_f32(value);
                    let u64ish = self.is_u64(target);
                    if !self.emit_f32_arith(assign_binop(op), f32ish, line)
                        && !self.emit_u64_arith(assign_binop(op), u64ish, line)
                    {
                        self.emit_arith(
                            assign_binop(op),
                            NumType::Unknown,
                            self.infer(value),
                            is_nonzero_const(value),
                            line,
                        );
                        // `xs[i] += n` wraps at the element type's width.
                        if let Some(ty) = self.sized_int_ty(target) {
                            self.emit_narrow(&ty, line);
                        }
                    }
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
                    // `s.f = 1` on a `float64` field stores a float, the same
                    // conversion the field's own literal `T{1}` performs.
                    let fty = self.field_ty(recv, field);
                    self.emit_typed(value, &fty)?;
                } else {
                    self.b.emit(Op::Dup2, line);
                    self.b.emit(Op::CallBuiltin(host::GFIELD_GET, 2), line);
                    self.expr(value)?;
                    let f32ish = self.is_f32(target) || self.is_f32(value);
                    let u64ish = self.is_u64(target);
                    if !self.emit_f32_arith(assign_binop(op), f32ish, line)
                        && !self.emit_u64_arith(assign_binop(op), u64ish, line)
                    {
                        self.emit_arith(
                            assign_binop(op),
                            NumType::Unknown,
                            self.infer(value),
                            is_nonzero_const(value),
                            line,
                        );
                        // `s.f += n` wraps at the field's declared width.
                        if let Some(ty) = self.sized_int_ty(target) {
                            self.emit_narrow(&ty, line);
                        }
                    }
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
        label: &Option<String>,
    ) -> Result<(), String> {
        // `for v := range ch` is not an indexed walk: it receives until the
        // channel is closed and drained. Nothing about the generic path applies,
        // so it gets its own loop.
        if self.type_name(iter).starts_with("chan ") {
            return self.compile_for_range_chan(key, iter, body, label);
        }

        let n = self.temp_counter;
        self.temp_counter += 1;
        let it = format!("$it{n}");
        let keys = format!("$keys{n}");
        let n_keys = format!("$n{n}");
        let i = format!("$i{n}");

        // $it = iter; $keys = GRANGE_KEYS($it); $i = 0
        //
        // `range` over an *array* walks a copy: Go evaluates the range
        // expression once, and for an array that evaluation is a value copy, so
        // a write to the array inside the loop is not seen by the remaining
        // iterations. Over a slice it is the shared handle, and a write *is*
        // seen — the same expression, two behaviours, decided by the static
        // type. (Not `emit_value`, which would also copy a struct: `range` over
        // a struct is not a thing Go has.)
        self.expr(iter)?;
        let iter_ty = self.type_name(iter);
        if array_elem_ty(&iter_ty).is_some() {
            self.emit_copy_for(&iter_ty);
        }
        self.emit_set(&it, 0);

        // A slice, a fixed-size array and an integer all iterate `0 … n-1`, so
        // the key is `$i` itself and the materialized key list is pure waste —
        // an n-element allocation built only to be read back one index at a
        // time. Skipping it is also what lets the body reach native code: the
        // tracing JIT refuses a trace containing any `CallBuiltin`, and
        // `$keys[$i]` was one per iteration.
        let indexed = integer_keyed_range(&iter_ty);
        if !indexed {
            self.emit_get(&it, 0);
            self.b.emit(Op::CallBuiltin(host::GRANGE_KEYS, 1), 0);
            self.emit_set(&keys, 0);
        }
        self.b.emit(Op::LoadInt(0), 0);
        self.emit_set(&i, 0);

        // The iteration count, read once: Go fixes it when the loop starts, so
        // an `append` in the body does not lengthen the walk and a `delete`
        // does not shorten it. For a slice or array that is `len`; for an
        // integer it is the value itself (a negative one runs zero times,
        // which the `$i < $n` test already says); for a map or string it is the
        // length of the key snapshot, which nothing in the body can reach.
        if int_range_ty(&iter_ty) {
            self.emit_get(&it, 0);
        } else {
            self.emit_get(if indexed { &it } else { &keys }, 0);
            self.b.emit(Op::CallBuiltin(host::GLEN, 1), 0);
        }
        self.types.insert(n_keys.clone(), NumType::Int);
        self.emit_set(&n_keys, 0);

        self.loops.push(LoopScope {
            label: label.clone(),
            ..Default::default()
        });
        // The entry guard: `$i < $n` decides whether the loop runs at all. The
        // same test is repeated below the body as a conditional backward branch
        // — the rotated shape [`Compiler::compile_for`] explains.
        self.emit_get(&i, 0);
        self.emit_get(&n_keys, 0);
        self.b.emit(Op::NumLt, 0);
        let guard = self.b.emit(Op::JumpIfFalse(0), 0);
        let top = self.b.current_pos();

        // key := $keys[$i] — or `$i` itself where the two are the same number.
        if let Some(k) = key {
            if indexed {
                self.emit_get(&i, 0);
            } else {
                self.emit_get(&keys, 0);
                self.emit_get(&i, 0);
                self.b.emit(Op::CallBuiltin(host::GINDEX_GET, 2), 0);
            }
            self.emit_set(k, 0);
            self.types.insert(k.clone(), NumType::Unknown);
        }
        // val := GRANGE_VAL($it, key)  — the loop value for the current key. This
        // indexes a slice/map element but decodes the rune of a string (Go ranges
        // strings by rune, so the value is a code point, not a byte).
        if let Some(v) = val {
            self.emit_get(&it, 0);
            if indexed {
                self.emit_get(&i, 0);
            } else {
                self.emit_get(&keys, 0);
                self.emit_get(&i, 0);
                self.b.emit(Op::CallBuiltin(host::GINDEX_GET, 2), 0);
            }
            self.b.emit(Op::CallBuiltin(host::GRANGE_VAL, 2), 0);
            // The range variable is a *copy* of the element, so `for _, v := range
            // xs { v.N = 1 }` leaves `xs` untouched — the single most damaging
            // place aliasing showed up, because Go programs rely on it to mean
            // "read-only walk".
            let elem = self.elem_type_of(&self.type_name(iter));
            self.emit_copy_for(&elem);
            self.emit_set(v, 0);
            self.types.insert(v.clone(), NumType::Unknown);
            // Only when the element type is actually known: recording an empty
            // one would clobber whatever an outer declaration of this name left
            // behind, which is what `emit_narrow` and the width tags read.
            if !elem.is_empty() {
                self.decl_types.insert(v.clone(), elem);
            }
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
        self.emit_get(&i, 0);
        self.emit_get(&n_keys, 0);
        self.b.emit(Op::NumLt, 0);
        self.b.emit(Op::JumpIfTrue(top), 0);
        let end = self.b.current_pos();

        let scope = self.loops.pop().unwrap();
        self.b.patch_jump(guard, end);
        for j in scope.continues {
            self.b.patch_jump(j, post_pos);
        }
        for j in scope.breaks {
            self.b.patch_jump(j, end);
        }
        Ok(())
    }

    /// Lower `for v := range ch`: receive until the channel is closed *and*
    /// drained, binding `v` to each value received. Go allows only the one
    /// range variable here, and it is the value, not an index.
    ///
    /// The termination test is the receive itself: the scheduler answers a
    /// drained closed channel with the sentinel (see `host::chan_closed_sentinel`),
    /// which is the same signal `v, ok := <-ch` reads. Testing "closed and
    /// empty" separately after the receive would be a race.
    fn compile_for_range_chan(
        &mut self,
        val: &Option<String>,
        chan: &Expr,
        body: &[Stmt],
        label: &Option<String>,
    ) -> Result<(), String> {
        let n = self.temp_counter;
        self.temp_counter += 1;
        let ch = format!("$rch{n}");
        let raw = format!("$rcv{n}");
        let elem = self.chan_elem_ty(chan);

        // Evaluate the channel once, as Go does.
        self.expr(chan)?;
        self.emit_set(&ch, 0);

        self.loops.push(LoopScope {
            label: label.clone(),
            ..Default::default()
        });
        let top = self.b.current_pos();
        self.emit_get(&ch, 0);
        self.b.emit(Op::ChanRecv, 0);
        self.types.insert(raw.clone(), NumType::Unknown);
        self.emit_set(&raw, 0);
        // Closed and drained ends the loop.
        self.emit_get(&raw, 0);
        self.b.emit(Op::CallBuiltin(host::GCHAN_OK, 1), 0);
        let jf = self.b.emit(Op::JumpIfFalse(0), 0);
        self.loops.last_mut().expect("loop scope").breaks.push(jf);
        if let Some(v) = val {
            self.emit_get(&raw, 0);
            self.emit_set(v, 0);
            self.types.insert(v.clone(), numtype_of_ty(&elem));
            self.decl_types.insert(v.clone(), elem);
        }

        for s in body {
            self.stmt(s)?;
        }

        // `continue` re-enters at the next receive, which is also the loop top.
        let post_pos = self.b.current_pos();
        self.b.emit(Op::Jump(top), 0);
        let end = self.b.current_pos();

        let scope = self.loops.pop().expect("loop scope");
        for j in scope.continues {
            self.b.patch_jump(j, post_pos);
        }
        for j in scope.breaks {
            self.b.patch_jump(j, end);
        }
        Ok(())
    }

    /// Emit the default zero value for a declared-without-initializer variable.
    /// Emit the zero value of the *written* type `ty`. A slice or map zero is
    /// Go's typed nil — it prints as `[]` / `map[]`, has length 0, is appendable
    /// (slice) and readable (map), and still compares equal to `nil` — which the
    /// erased [`NumType`] cannot express, since it collapses `[]T`, `map[K]V` and
    /// `any` into one `Unknown`. Everything else falls through to [`Self::emit_default`].
    fn emit_zero(&mut self, ty: &str, line: u32) {
        // A defined type's zero value is its base's: `var s mySlice` is a nil
        // slice, which prints `[]` and appends, not the scalar zero. (Go rejects
        // a `type` cycle, so following the chain terminates.)
        if let Some(base) = self.defined_types.get(ty).cloned() {
            if base != ty {
                self.emit_zero(&base, line);
                return;
            }
        }
        // A fixed-size array's zero is N element zeros, not nil: `var a [3]int`
        // is `[0 0 0]` and `type s struct{ a [2]string }` zeroes to `{[ ]}`.
        if let (Some(elem), Some(n)) = (array_elem_ty(ty), array_len_of(ty)) {
            let (elem, n) = (elem.to_string(), n);
            let c = self.b.add_constant(Value::str("slice"));
            self.b.emit(Op::LoadConst(c), line);
            self.b.emit(Op::LoadInt(n as i64), line);
            if self.structs.contains(&elem) {
                let _ = self.struct_lit(&elem, &[]);
            } else {
                self.emit_zero(&elem, line);
            }
            self.b.emit(Op::LoadInt(-1), line);
            let ec = self.b.add_constant(Value::str(elem));
            self.b.emit(Op::LoadConst(ec), line);
            self.b.emit(Op::CallBuiltin(host::GMAKE, 5), line);
            // The zero value is another of the places an array is born, so it
            // too carries the written type onto the object for `%T`/`%#v`.
            self.emit_array_tag(ty, line);
            return;
        }
        if ty.starts_with("[]") || ty.starts_with("map[") {
            let c = self.b.add_constant(Value::str(ty.to_string()));
            self.b.emit(Op::LoadConst(c), line);
            self.b.emit(Op::CallBuiltin(host::GNIL_OF, 1), line);
            return;
        }
        self.emit_default(numtype_of_ty(ty), line);
    }

    /// Emit the extra argument a map index takes: the value a *missing* key
    /// yields. Go's answer is the value type's zero — `""` for a
    /// `map[K]string`, `false` for a `map[K]bool`, a nil slice, a nil pointer,
    /// a zero struct — and the host cannot work any of that out. It sees a key
    /// that is not there and a map that does not carry its value type, so its
    /// own default is the integer `0`, which is right for exactly the numeric
    /// case. The value type is known here and nowhere else.
    ///
    /// Answers the argument count to call the builtin with: 3 when a zero was
    /// emitted, 2 when the receiver is not a statically known map — a slice or
    /// string index, or an expression whose type this pass could not name — and
    /// the host's default stands.
    ///
    /// The argument is evaluated before the call, so a struct value type costs
    /// one zero-struct construction per lookup whether the key is there or not.
    /// Every other value type's zero is a single constant-load op.
    /// Whether `ty` names a Go **comparable** type, which is the only kind a
    /// map key may have.
    ///
    /// A slice, a map and a function are not comparable, and neither is a
    /// struct or an array built out of one. A pointer is (Go compares the
    /// address), a channel is, and an interface is *statically* — the check Go
    /// makes on the dynamic type of an interface key happens at run time.
    fn comparable(&self, ty: &str) -> bool {
        let ty = ty.trim();
        if ty.starts_with('*') {
            return true;
        }
        if ty.starts_with("[]") || ty.starts_with("map[") || ty.starts_with("func(") || ty == "func"
        {
            return false;
        }
        if let Some(elem) = array_elem_ty(ty) {
            return self.comparable(elem);
        }
        // A defined type is comparable exactly when its underlying type is.
        if let Some(base) = self.defined_types.get(ty) {
            if base != ty {
                return self.comparable(base);
            }
        }
        match self.struct_fields.get(ty) {
            Some(fields) => fields.iter().all(|(_, t)| self.comparable(t)),
            None => true,
        }
    }

    /// Reject a map key type Go rejects. `go` refuses to build the program at
    /// all — `invalid map key type []int` — so go-rs does too rather than
    /// silently building a map whose keys nothing can look up.
    fn check_map_key(&self, key_ty: &str, line: u32) -> Result<(), String> {
        if self.comparable(key_ty) {
            return Ok(());
        }
        // A composite-literal type carries no line, so `0` means "unknown" and
        // the suffix is left off rather than pointing at the top of the file.
        let where_ = match line {
            0 => String::new(),
            n => format!(" (line {n})"),
        };
        Err(format!("go-rs: invalid map key type {key_ty}{where_}"))
    }

    /// The declared type of `recv.field`, or `""` when the receiver's struct
    /// type is not known here.
    fn field_ty(&self, recv: &Expr, field: &str) -> String {
        let rt = self.type_name(recv);
        self.struct_fields
            .get(&rt)
            .and_then(|fs| fs.iter().find(|(n, _)| n == field))
            .map(|(_, t)| t.clone())
            .unwrap_or_default()
    }

    /// The underlying type of `ty` — a defined type resolves to its base, so
    /// `type celsius float64` answers `float64`. (Go rejects a `type` cycle, so
    /// following the chain terminates.)
    fn underlying(&self, ty: &str) -> String {
        match self.defined_types.get(ty) {
            Some(base) if base != ty => self.underlying(base),
            _ => ty.to_string(),
        }
    }

    /// Whether `ty` is a float type, following defined types to their base.
    fn is_float_ty(&self, ty: &str) -> bool {
        numtype_of_ty(&self.underlying(&base_type(ty))) == NumType::Float
    }

    /// Emit one call argument, converted to the parameter's declared type.
    ///
    /// Go converts an untyped constant to the parameter's type at the call, so
    /// `f(1)` on a `func f(x float64)` passes a `float64`. Without this the
    /// callee's parameter holds a `Value::Int` that only its *static* type says
    /// is a float — enough for arithmetic, which reads the static type, and not
    /// enough for `%T` or for a map key, which read the value.
    fn emit_arg(&mut self, a: &Expr, param_ty: Option<&String>) -> Result<(), String> {
        match param_ty {
            Some(t) if self.is_float_ty(t) => {
                let t = self.underlying(&base_type(t));
                self.emit_typed(a, &t)
            }
            _ => self.emit_value(a),
        }
    }

    /// Emit `m[k]`'s key, converted to the map's declared key type.
    ///
    /// An untyped constant in a `map[float64]V` index — `f[1]` — is a `float64`
    /// in Go, and once it has been pushed as a `Value::Int` nothing at run time
    /// can tell it from the integer it now looks like. The conversion therefore
    /// has to happen where the declared key type is known, which is here.
    ///
    /// Only a float key type needs it. Every other conversion [`Self::emit_typed`]
    /// would add is the identity or a struct copy, and copying a key per
    /// *lookup* is exactly the work the hash index was added to avoid.
    fn emit_map_key(&mut self, recv: &Expr, index: &Expr) -> Result<(), String> {
        match map_key_ty(&self.type_name(recv)) {
            Some(k) if self.is_float_ty(k) => {
                let k = self.underlying(&base_type(k));
                self.emit_typed(index, &k)
            }
            _ => self.expr(index),
        }
    }

    fn emit_map_miss_zero(&mut self, recv: &Expr, line: u32) -> Result<u8, String> {
        let Some(v_ty) = map_value_ty(&self.type_name(recv)).map(str::to_string) else {
            return Ok(2);
        };
        if v_ty.is_empty() {
            return Ok(2);
        }
        self.emit_elem_zero(&v_ty, line)?;
        Ok(3)
    }

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
                    // `&T{…}` (and `new(T)`, which parses to it) allocates: the
                    // result is a pointer, which Go compares by address, not by
                    // field. Mark the fresh handle so `==` knows. Taking the
                    // address of an existing variable shares its handle, which
                    // must keep comparing as the value it already is.
                    if matches!(op, UnOp::Addr) && matches!(**rhs, Expr::StructLit { .. }) {
                        self.b.emit(Op::CallBuiltin(host::GPTR_MARK, 1), 0);
                    }
                } else if matches!(op, UnOp::Neg) && matches!(**rhs, Expr::Float(f, _) if f == 0.0)
                {
                    // Go's untyped constants have no signed zero, so the constant
                    // `-0.0` is exactly `0` and prints as `0`, not `-0`. Negating
                    // at run time would produce IEEE −0.0.
                    self.b.emit(Op::LoadFloat(0.0), 0);
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
                    // `^uint8(0)` is 255, not -1: a sized operand wraps the
                    // result at its own width.
                    if let Some(ty) = self.sized_int_ty(e) {
                        self.emit_narrow(&ty, 0);
                    }
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
                // An interface with a method set is satisfied by method set, not
                // by type identity, so it asserts through its own builtin (whose
                // panic names the missing method, as Go's does).
                match self.iface_of(ty) {
                    Some(ms) => {
                        let want = self.b.add_constant(Value::str(ms.join(",")));
                        self.b.emit(Op::LoadConst(want), 0);
                        let disp = self.b.add_constant(Value::str(iface_display(ty)));
                        self.b.emit(Op::LoadConst(disp), 0);
                        self.b.emit(Op::CallBuiltin(host::GASSERT_IFACE, 3), 0);
                    }
                    None => {
                        let c = self.b.add_constant(Value::str(type_to_tag(ty)));
                        self.b.emit(Op::LoadConst(c), 0);
                        self.b.emit(Op::CallBuiltin(host::GASSERT, 2), 0);
                    }
                }
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
                    // `strconv.ErrSyntax` / `strconv.ErrRange` are error *values*,
                    // not constants: `errors.Is` compares them by pointer, so each
                    // mention must yield the one handle the host memoizes rather
                    // than a fresh chunk constant.
                    if pkg == "strconv" && matches!(field.as_str(), "ErrSyntax" | "ErrRange") {
                        let c = self.b.add_constant(Value::str(field.clone()));
                        self.b.emit(Op::LoadConst(c), 0);
                        self.b
                            .emit(Op::CallBuiltin(host::stdlib::STRCONV_ERR, 1), 0);
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
                self.emit_map_key(recv, index)?;
                let argc = self.emit_map_miss_zero(recv, 0)?;
                self.b.emit(Op::CallBuiltin(host::GINDEX_GET, argc), 0);
                self.emit_panic_check(0); // index out of range is recoverable
            }
            Expr::Slice {
                recv,
                low,
                high,
                max,
            } => {
                // `recv[low:high:max]`: push recv, low, high, max — each omitted
                // bound as `-1` (0 / len / cap respectively).
                self.expr(recv)?;
                for bound in [low, high, max] {
                    match bound {
                        Some(e) => self.expr(e)?,
                        None => {
                            self.b.emit(Op::LoadInt(-1), 0);
                        }
                    }
                }
                self.b.emit(Op::CallBuiltin(host::GSLICE_SUB, 4), 0);
            }
            Expr::SliceLit {
                elem_ty,
                elems,
                array_len,
            } => {
                let (elem_ty, elems) = (elem_ty.clone(), elems.clone());
                self.emit_lit_chunked(host::GSLICE_LIT, 0, 1, elems.len(), 0, |c, i| {
                    c.emit_typed(&elems[i], &elem_ty)
                })?;
                // `[N]T{…}` is one of the three places an array value is born,
                // so it carries the written type onto the object for `%T`/`%#v`.
                if let Some(n) = array_len {
                    self.emit_array_tag(&format!("[{n}]{elem_ty}"), 0);
                }
            }
            Expr::MapLit {
                key_ty,
                val_ty,
                pairs,
            } => {
                let (key_ty, val_ty, pairs) = (key_ty.clone(), val_ty.clone(), pairs.clone());
                self.check_map_key(&key_ty, 0)?;
                // The key is typed by the map's *underlying* key type, so a
                // `map[celsius]V{1: …}` stores the same `float64` a `c[1]`
                // index looks up — a defined type and its base have to agree
                // about which of the two a literal `1` becomes.
                let key_ty = self.underlying(&base_type(&key_ty));
                self.emit_lit_chunked(host::GMAP_LIT, 0, 2, pairs.len(), 0, |c, i| {
                    let (k, v) = &pairs[i];
                    c.emit_typed(k, &key_ty)?;
                    c.emit_typed(v, &val_ty)
                })?;
            }
            Expr::StructLit { type_name, fields } => self.struct_lit(type_name, fields)?,
            Expr::Make {
                is_map,
                len,
                cap,
                elem_zero,
                elem_ty,
            } => {
                if *is_map {
                    // `elem_ty` of a `make(map[K]V)` is the whole written type.
                    if let Some(k) = map_key_ty(elem_ty) {
                        self.check_map_key(k, 0)?;
                    }
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
                    // `cap` defaults to `len`; `-1` tells the host "omitted".
                    match cap {
                        Some(e) => self.expr(e)?,
                        None => {
                            self.b.emit(Op::LoadInt(-1), 0);
                        }
                    }
                    // The element type, so replicating the one zero gives each
                    // slot its own value when the element is itself a value type
                    // (`make([][2]int, n)`, `var a [3]pt`).
                    let ec = self.b.add_constant(Value::str(elem_ty.clone()));
                    self.b.emit(Op::LoadConst(ec), 0);
                    self.b.emit(Op::CallBuiltin(host::GMAKE, 5), 0);
                }
            }
            Expr::MakeChan { cap, .. } => {
                match cap {
                    Some(e) => self.expr(e)?,
                    None => {
                        self.b.emit(Op::LoadInt(0), 0);
                    }
                }
                self.b.emit(Op::ChanMake, 0);
            }
            Expr::Recv { chan } => self.emit_recv(chan, 0)?,
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
        let given = given.to_vec();
        // The type name is the one fixed stack value `GSTRUCT_NEW` spends
        // before its `name,value` field pairs.
        self.emit_lit_chunked(host::GSTRUCT_NEW, 1, 2, decl.len(), 0, |c, i| {
            let (fname, fty) = &decl[i];
            let fc = c.b.add_constant(Value::str(fname.clone()));
            c.b.emit(Op::LoadConst(fc), 0);
            let value: Option<&Expr> = if keyed {
                given
                    .iter()
                    .find(|(k, _)| k.as_deref() == Some(fname))
                    .map(|(_, v)| v)
            } else {
                given.get(i).map(|(_, v)| v)
            };
            match value {
                Some(e) => c.emit_typed(e, fty),
                // A struct-typed field's zero value is a zero struct, not nil —
                // `var c counter` gives `c.mu` a usable `sync.Mutex`. A pointer
                // field (`*T`) is nil, so only the bare type recurses.
                None if c.structs.contains(fty) => c.struct_lit(fty, &[]),
                None => {
                    c.emit_zero(fty, 0);
                    Ok(())
                }
            }
        })
    }

    /// Emit the right-hand side of a binding to `name`, additionally tracking
    /// when `name` becomes a statically-known closure (so a later `name(args)`
    /// dispatches directly).
    fn emit_rhs(&mut self, name: &str, e: &Expr) -> Result<(), String> {
        // `s = nil` on a slice- or map-typed variable rebinds it to that type's
        // typed nil, so it goes on printing `[]` / `map[]` rather than `<nil>`.
        if self.is_nil_literal(e) {
            if let Some(ty) = self.decl_types.get(name).cloned() {
                if ty.starts_with("[]") || ty.starts_with("map[") {
                    self.emit_zero(&ty, 0);
                    return Ok(());
                }
            }
        }
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

    /// Emit `e` converted to the declared type `ty`, when that conversion is one
    /// Go's assignability rules perform implicitly.
    ///
    /// The case that matters is an untyped integer constant landing in a float
    /// slot: `[]float64{1, 2}`, `map[string]float64{"k": 3}`, `S{F: 3}`,
    /// `var x float64 = 3`. Go converts each to a `float64`, so the value is a
    /// float for every later operation. go-rs used to store the raw integer,
    /// which made `xs[0] / 2` take the integer-division path and made
    /// `[]float64{1e6}` print as `1000000` instead of `1e+06`.
    /// Whether `e` is the predeclared `nil` — a bare `nil` that no local or
    /// global shadows. It has no `Expr` variant of its own: it lowers as an
    /// unbound identifier, whose empty slot reads back as `Value::Undef`.
    fn is_nil_literal(&self, e: &Expr) -> bool {
        matches!(e, Expr::Ident(n)
            if n == "nil" && !self.types.contains_key(n) && !self.globals.contains(n))
    }

    /// Emit `return`'s i-th value, typed by the function's i-th declared result.
    fn emit_result(&mut self, e: &Expr, i: usize, results: &[String]) -> Result<(), String> {
        match results.get(i) {
            Some(ty) => self.emit_typed(e, ty),
            None => self.emit_value(e),
        }
    }

    fn emit_typed(&mut self, e: &Expr, ty: &str) -> Result<(), String> {
        // A written `nil` for a slice or map type is that type's typed nil, not
        // the untyped one — `var s []int = nil` prints `[]`, like `var s []int`.
        if self.is_nil_literal(e) && (ty.starts_with("[]") || ty.starts_with("map[")) {
            self.emit_zero(ty, 0);
            return Ok(());
        }
        // A defined type's destination is its base's: `type celsius float64`
        // takes an untyped constant as a `float64`, and a `type f32 float32`
        // rounds to 32 bits.
        let ty = &self.underlying(&base_type(ty));
        // A `float32` destination rounds to 32 bits — `var f float32 = 1.0/3.0`
        // holds the `f32` nearest one third, not the `f64` one.
        if ty == "float32" && !self.is_f32(e) {
            self.emit_value(e)?;
            let c = self.b.add_constant(Value::str("float32"));
            self.b.emit(Op::LoadConst(c), 0);
            self.b.emit(Op::CallBuiltin(host::GCONV, 2), 0);
            return Ok(());
        }
        if numtype_of_ty(ty) != NumType::Float || self.infer(e) != NumType::Int {
            return self.emit_value(e);
        }
        // A literal converts at compile time; anything else goes through the
        // runtime conversion builtin `float64(x)` uses.
        if let Expr::Int(n) = e {
            self.b.emit(Op::LoadFloat(*n as f64), 0);
            return Ok(());
        }
        self.emit_value(e)?;
        let c = self.b.add_constant(Value::str("float64"));
        self.b.emit(Op::LoadConst(c), 0);
        self.b.emit(Op::CallBuiltin(host::GCONV, 2), 0);
        Ok(())
    }

    /// Emit `value`, then a `GSTRUCT_COPY` if its static type is a struct — Go
    /// copies a struct on assignment / parameter bind / return (slices and maps
    /// are reference types and pass through the copy unchanged).
    fn emit_value(&mut self, e: &Expr) -> Result<(), String> {
        self.expr(e)?;
        // `&x` is a pointer (a reference), so it is never copied.
        if matches!(e, Expr::Unary { op: UnOp::Addr, .. }) {
            return Ok(());
        }
        // `*p` asks for the pointed-to *value*, so it copies even though the
        // handle it evaluates to is a pointer. This is the one site where the
        // bind-time rule is inverted, and it is why the two cannot share an op:
        // `q := p` and `v := *p` reach the run time as the same handle, and only
        // the AST still knows which is which.
        let ty = self.type_name(e);
        if matches!(
            e,
            Expr::Unary {
                op: UnOp::Deref,
                ..
            }
        ) && self.structs.contains(&ty)
        {
            self.b.emit(Op::CallBuiltin(host::GSTRUCT_COPY, 1), 0);
            return Ok(());
        }
        self.emit_copy_for(&ty);
        Ok(())
    }

    /// Stamp the written `[N]T` on the fixed-size array on top of the stack, so
    /// `%T` and `%#v` can name it — an array and a slice are the same heap
    /// object, and the length is not recoverable from the elements.
    ///
    /// Only the three places an array is *born* need this: a composite literal
    /// (`[N]T{…}`), a zero value ([`Self::emit_zero`], which covers a struct
    /// field and a named result), and a `var a [N]T` declaration — whose
    /// initializer-less form lowers to a `make` that carries no type of its own.
    /// Every other array is a copy of one of those, and [`host::GARRAY_COPY`]
    /// carries the tag across.
    fn emit_array_tag(&mut self, ty: &str, line: u32) {
        let shown = self.go_type_display(ty);
        let c = self.b.add_constant(Value::str(shown));
        self.b.emit(Op::LoadConst(c), line);
        self.b.emit(Op::CallBuiltin(host::GARRAY_TAG, 2), line);
    }

    /// A written type as Go's `fmt` spells it: a declared type is qualified by
    /// its package (go-rs only compiles `package main`), and the two spelling
    /// aliases are resolved — `byte` is `uint8` and `rune` is `int32`.
    ///
    /// The rewrite is structural, so a name nested any distance inside a
    /// composite is qualified too: `[2]map[string]pt` shows as
    /// `[2]map[string]main.pt`. `%T` on a struct reads the same qualification
    /// off the object ([`host::go_type_name`]), so the two agree.
    fn go_type_display(&self, ty: &str) -> String {
        let ty = ty.trim();
        if let (Some(elem), Some(n)) = (array_elem_ty(ty), array_len_of(ty)) {
            return format!("[{n}]{}", self.go_type_display(elem));
        }
        if let Some(elem) = ty.strip_prefix("[]") {
            return format!("[]{}", self.go_type_display(elem));
        }
        if let Some(elem) = ty.strip_prefix('*') {
            return format!("*{}", self.go_type_display(elem));
        }
        if let Some(rest) = ty.strip_prefix("map[") {
            // The key may itself be a composite, so the key's own brackets have
            // to be balanced off before the closing one is the map's.
            let mut depth = 0usize;
            for (i, c) in rest.char_indices() {
                match c {
                    '[' => depth += 1,
                    ']' if depth == 0 => {
                        return format!(
                            "map[{}]{}",
                            self.go_type_display(&rest[..i]),
                            self.go_type_display(&rest[i + 1..])
                        );
                    }
                    ']' => depth -= 1,
                    _ => {}
                }
            }
            return ty.to_string();
        }
        match ty {
            "byte" => "uint8".to_string(),
            "rune" => "int32".to_string(),
            "any" | "interface{}" => "interface {}".to_string(),
            // The parser erases a function type's signature, and `%T` on a
            // closure names it `func()` for the same reason — so an array of
            // them agrees with the element it holds.
            "func" => "func()".to_string(),
            // A declared type — a struct or a defined type over any other base —
            // is qualified by its package.
            _ if self.structs.contains(ty) || self.defined_types.contains_key(ty) => {
                format!("main.{ty}")
            }
            _ => ty.to_string(),
        }
    }

    /// `n` as the arity byte a `CallBuiltin` carries, or a compile error.
    ///
    /// fusevm holds the count in a `u8`, and a call site — unlike a composite
    /// literal — has no container to build up in chunks, so an over-long one
    /// cannot be lowered. Wrapping the count instead would drop arguments
    /// silently: `fmt.Println` with 256 of them printed a blank line. Refusing
    /// to build is the honest answer; Go itself puts no limit here, so this is
    /// a stated go-rs bound rather than a diagnosis of the program.
    fn call_arity(n: usize, what: &str, line: u32) -> Result<u8, String> {
        u8::try_from(n).map_err(|_| {
            format!(
                "go-rs: `{what}` takes at most {} arguments here, got {n} (line {line})",
                u8::MAX
            )
        })
    }

    /// Emit a composite literal of `items` items, `slots` stack values each,
    /// splitting it across as many calls as fusevm's `u8` arity byte requires.
    ///
    /// One `Op::CallBuiltin` carries at most 255 stack values, `fixed` of which
    /// the literal builtin already spends on something else (the struct
    /// literal's type name). A Go literal has no such bound — `[]int{…}` with
    /// 256 elements is ordinary — so the first chunk goes to `lit` and each
    /// later one to [`host::GLIT_EXTEND`], which appends to the container the
    /// first call left on the stack. Emitting the count as a plain `as u8`
    /// wrapped it instead, and the literal silently lost everything past the
    /// first 255 values.
    fn emit_lit_chunked(
        &mut self,
        lit: u16,
        fixed: usize,
        slots: usize,
        items: usize,
        line: u32,
        mut emit_item: impl FnMut(&mut Self, usize) -> Result<(), String>,
    ) -> Result<(), String> {
        const ARITY_MAX: usize = u8::MAX as usize;
        let head = items.min((ARITY_MAX - fixed) / slots);
        for i in 0..head {
            emit_item(self, i)?;
        }
        self.b
            .emit(Op::CallBuiltin(lit, (fixed + head * slots) as u8), line);
        // Each later chunk spends one slot on the container it extends.
        let per_chunk = (ARITY_MAX - 1) / slots;
        let mut done = head;
        while done < items {
            let k = (items - done).min(per_chunk);
            for i in done..done + k {
                emit_item(self, i)?;
            }
            self.b.emit(
                Op::CallBuiltin(host::GLIT_EXTEND, (1 + k * slots) as u8),
                line,
            );
            done += k;
        }
        Ok(())
    }

    /// Emit the value-copy a Go *value* type needs, for a value already on the
    /// stack whose static type is `ty`. A no-op for every reference type.
    ///
    /// Go has exactly two composite value types: the struct and the fixed-size
    /// array. Both are shared heap handles in go-rs, so both need an explicit
    /// copy at each site Go copies; slices, maps, channels, pointers and
    /// functions are references and must keep their handle.
    ///
    /// The array copy carries its *element* type, because the recursion cannot
    /// be decided at run time: an array and a slice are the same heap object, so
    /// only the written type says whether an element is itself an array (copy)
    /// or a slice (share).
    fn emit_copy_for(&mut self, ty: &str) {
        if let Some(elem) = array_elem_ty(ty) {
            let c = self.b.add_constant(Value::str(elem.to_string()));
            self.b.emit(Op::LoadConst(c), 0);
            self.b.emit(Op::CallBuiltin(host::GARRAY_COPY, 2), 0);
        } else if self.structs.contains(ty) {
            // A *bind*: copies a struct value, shares a pointer. See
            // `host::struct_bind` for why the two cannot be one op.
            self.b.emit(Op::CallBuiltin(host::GSTRUCT_BIND, 1), 0);
        }
    }

    /// Copy the receiver already on the stack when `ty.method` was declared with
    /// a value receiver. Go binds `func (t T) m()` to a copy — `a.m()` cannot
    /// change `a` — while `func (t *T) m()` binds the struct itself, which is the
    /// shared handle go-rs already has.
    fn emit_recv_copy(&mut self, ty: &str, method: &str) {
        if self
            .value_recv_methods
            .contains(&(ty.to_string(), method.to_string()))
        {
            self.b.emit(Op::CallBuiltin(host::GSTRUCT_COPY, 1), 0);
        }
    }

    /// Lower a method call `recv.method(args)`. The receiver's static type names
    /// the method set; the receiver is passed as the first (deepest) argument.
    /// A *pointer*-receiver method gets the caller's own struct handle, so a
    /// field it writes is observed by the caller; a value-receiver one gets a
    /// copy ([`Self::emit_recv_copy`]) and cannot write through.
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
            self.emit_recv_copy(&ty, method);
            let param_tys = self
                .method_param_tys
                .get(&(ty.clone(), method.to_string()))
                .cloned()
                .unwrap_or_default();
            for (i, a) in args.iter().enumerate() {
                self.emit_arg(a, param_tys.get(i))?;
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
        // A call on an interface-typed receiver is legal as long as the interface
        // declares the method — whether any concrete type in the program
        // implements it is irrelevant, because the assertion that produced the
        // receiver can then never succeed. (`errors.Is` calls `x.Is(target)` on
        // an `interface{ Is(error) bool }` binding; most programs define no such
        // method, and Go still compiles the call.) Anything else is a real
        // unknown-method error.
        let declared_by_iface = self
            .iface_of(&ty)
            .is_some_and(|ms| ms.iter().any(|m| m.starts_with(&format!("{method}/"))));
        if candidates.is_empty() && !declared_by_iface {
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
            // The dispatch arm knows the concrete type, so the value-vs-pointer
            // receiver question is answerable here even though the static type
            // was not.
            self.emit_recv_copy(t, method);
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
                Expr::Selector { recv, field } => {
                    // A native package function that returns `(value, error)`.
                    if let Expr::Ident(pkg) = recv.as_ref() {
                        if host::stdlib::returns_error(pkg, field) {
                            return Some(2);
                        }
                        // `fmt.Fprint*` is rewritten to the writer's own
                        // `Write`, so it returns what `Write` does — but the
                        // count is asked for before the rewrite, off the name
                        // the program wrote.
                        if pkg == "fmt"
                            && matches!(field.as_str(), "Fprintf" | "Fprint" | "Fprintln")
                        {
                            return Some(2);
                        }
                    }
                    // A method call `recv.M()` — look up M's result count on the
                    // receiver's static type. (A package call like `strings.Split`
                    // has an untyped receiver, so this yields `None`.)
                    let rt = self.type_name(recv);
                    if !rt.is_empty() {
                        if let Some(n) = self.method_nresults.get(&(rt.clone(), field.clone())) {
                            return Some(*n);
                        }
                        // An *interface*-typed receiver declares no method of
                        // its own, so the count comes off the method set, whose
                        // entries carry the result types (`Write/1:int,error`).
                        // Without this a call through an interface looked like
                        // it yielded one value, and `n, err := w.Write(p)` put
                        // the whole tuple in `n`.
                        let prefix = format!("{field}/");
                        if let Some(sig) = self
                            .iface_of(&rt)
                            .and_then(|ms| ms.iter().find(|m| m.starts_with(&prefix)))
                        {
                            let results = sig.split_once(':').map(|(_, r)| r).unwrap_or("");
                            return Some(match results.is_empty() {
                                true => 0,
                                false => results.split(',').count(),
                            });
                        }
                        return None;
                    }
                    // A *source*-linked package's function is merged into the
                    // program under its qualified name, so `io.WriteString`'s
                    // two results are known the same way a local `func`'s are.
                    // Reached only when the receiver named no type, which is
                    // what tells a package selector from a method call.
                    if let Expr::Ident(pkg) = recv.as_ref() {
                        return self
                            .funcs
                            .get(&format!("{pkg}.{field}"))
                            .map(|sig| sig.nresults);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Reject a multiple-value call used where one value is expected — Go's
    /// "multiple-value f() in single-value context". go-rs's tuple is a slice
    /// heap value, so without this the operand would silently be that slice.
    fn check_single_value(&self, e: &Expr, line: u32) -> Result<(), String> {
        if self.call_result_count(e).is_some_and(|n| n >= 2) {
            let name = match e {
                Expr::Call { func, .. } => match func.as_ref() {
                    Expr::Ident(n) => n.clone(),
                    Expr::Selector { recv, field } => match recv.as_ref() {
                        Expr::Ident(p) => format!("{p}.{field}"),
                        _ => field.clone(),
                    },
                    _ => String::new(),
                },
                _ => String::new(),
            };
            // Expression lowering carries no line of its own; only report one
            // when the caller had it.
            let at = if line == 0 {
                String::new()
            } else {
                format!(" (line {line})")
            };
            return Err(format!(
                "go-rs: multiple-value {name}() in single-value context{at}"
            ));
        }
        Ok(())
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
                // `[]byte(s)` / `[]rune(s)` name the slice type they convert to,
                // which is what tells `fmt` the result is text rather than a
                // list of numbers.
                Expr::Ident(name) if matches!(name.as_str(), "[]byte" | "[]rune") => name.clone(),
                // A conversion to a defined type has that type, which is the
                // only place its name enters the value flow: `Weekday(3)` is an
                // `int` at run time and a `main.Weekday` to `%T`.
                Expr::Ident(name) if self.defined_types.contains_key(name) => name.clone(),
                Expr::Ident(name) => self
                    .funcs
                    .get(name)
                    .map(|s| base_type(&s.result_ty))
                    .unwrap_or_default(),
                _ => String::new(),
            },
            // A slice literal names its own type, so a variable bound to one
            // records `[]T` and an element's declared type is recoverable.
            // A slice literal names its own reference type; an array literal
            // names the *value* type `[N]T`, which is what makes a binding to
            // one copy at every site Go copies.
            Expr::SliceLit {
                elem_ty,
                array_len: Some(n),
                ..
            } => format!("[{n}]{elem_ty}"),
            Expr::SliceLit { elem_ty, .. } => format!("[]{elem_ty}"),
            Expr::Make {
                is_map, elem_ty, ..
            } if !is_map => format!("[]{elem_ty}"),
            // A map literal / `make(map[K]V)` names its own type the same way, so
            // an element's declared type is recoverable from the variable.
            Expr::MapLit { key_ty, val_ty, .. } => format!("map[{key_ty}]{val_ty}"),
            // A map `make` records the whole written type in `elem_ty`.
            Expr::Make { elem_ty, .. } => elem_ty.clone(),
            // `make(chan T)` names `chan T`, so a variable bound to one keeps the
            // element type a closed receive needs the zero value of.
            Expr::MakeChan { elem_ty, .. } => format!("chan {elem_ty}"),
            // `<-ch` has the channel's element type.
            Expr::Recv { chan } => self.chan_elem_ty(chan),
            // `s[i]` / `m[k]` has the container's element type. Naming it is what
            // makes an indexed read of a struct element copy (Go value
            // semantics): `e := xs[0]; e.N = 1` must not write through to `xs[0]`.
            Expr::Index { recv, .. } => self.elem_type_of(&self.type_name(recv)),
            _ => String::new(),
        }
    }

    /// The element type of a written container type: `[]T` and `map[K]V` name
    /// `T` and `V`. Anything else has no element type.
    fn elem_type_of(&self, container: &str) -> String {
        if let Some(elem) = array_elem_ty(container) {
            return base_type(elem);
        }
        if let Some(elem) = container.strip_prefix("[]") {
            return base_type(elem);
        }
        base_type(map_value_ty(container).unwrap_or(""))
    }

    /// Emit one operand of a comparison. `*p` loads the pointed-to struct as a
    /// *value*, so `*a == *b` compares fields even when `a` and `b` are distinct
    /// pointers; go-rs's deref is a no-op on the shared handle, so the load is
    /// made explicit here (the copy is discarded right after the compare).
    fn emit_compare_operand(&mut self, e: &Expr) -> Result<(), String> {
        self.check_single_value(e, 0)?;
        self.expr(e)?;
        if matches!(
            e,
            Expr::Unary {
                op: UnOp::Deref,
                ..
            }
        ) {
            self.b.emit(Op::CallBuiltin(host::GSTRUCT_COPY, 1), 0);
        }
        Ok(())
    }

    /// Lower `errors.As(err, &target)`: walk `err`'s tree for an error whose
    /// dynamic type is `target`'s, assign it to `target` on a hit, and leave the
    /// hit/miss bool on the stack (the call sits in expression position).
    ///
    /// Go reads the target's type off the `*T` pointer with reflectlite and
    /// stores through it; go-rs takes the type from the target's declaration and
    /// assigns the variable directly, which needs no reflection and no pointer
    /// write-back. A target whose type go-rs cannot name is rejected rather than
    /// silently never matching.
    fn errors_as(&mut self, args: &[Expr], line: u32) -> Result<(), String> {
        let [err, target] = args else {
            return Err(format!(
                "go-rs: errors.As takes 2 arguments, got {} (line {line})",
                args.len()
            ));
        };
        let Expr::Unary {
            op: UnOp::Addr,
            rhs: target,
        } = target
        else {
            return Err(format!(
                "go-rs: errors.As: second argument must be `&target` (line {line})"
            ));
        };
        let ty = self.type_name(target);
        if ty.is_empty() {
            return Err(format!(
                "go-rs: errors.As: cannot determine the target's type (line {line})"
            ));
        }
        let n = self.temp_counter;
        self.temp_counter += 1;
        let tup = format!("$as{n}");
        let ok = format!("$asok{n}");
        self.call(
            &Expr::Ident("errors.asTag".to_string()),
            &[err.clone(), Expr::Str(type_to_tag(&ty))],
            false,
            line,
        )?;
        self.types.insert(tup.clone(), NumType::Unknown);
        self.emit_set(&tup, line);
        // ok = tuple[1]
        self.emit_get(&tup, line);
        self.b.emit(Op::LoadInt(1), line);
        self.b.emit(Op::CallBuiltin(host::GINDEX_GET, 2), line);
        self.types.insert(ok.clone(), NumType::Bool);
        self.emit_set(&ok, line);
        // Go leaves the target untouched on a miss.
        self.emit_get(&ok, line);
        let skip = self.b.emit(Op::JumpIfFalse(0), line);
        let hit = format!("$ashit{n}");
        self.emit_get(&tup, line);
        self.b.emit(Op::LoadInt(0), line);
        self.b.emit(Op::CallBuiltin(host::GINDEX_GET, 2), line);
        self.types.insert(hit.clone(), NumType::Unknown);
        self.decl_types.insert(hit.clone(), ty);
        self.emit_set(&hit, line);
        self.assign(target, AssignOp::Set, &Expr::Ident(hit), line)?;
        let end = self.b.current_pos();
        self.b.patch_jump(skip, end);
        self.emit_get(&ok, line);
        Ok(())
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

        // An `==`/`!=` with an interface operand is decided by dynamic type
        // before value, which neither native op can do: `Op::NumEq` on an `int`
        // beside a `float64` is answered inside fusevm by promoting the integer,
        // so the frontend is never asked. Route it to the builtin that holds
        // Go's rule. Only `==`/`!=` — an interface is unordered, so `<` on two
        // of them is a Go compile error and never reaches here.
        if matches!(op, BinOp::Eq | BinOp::Ne) && (self.is_iface(lhs) || self.is_iface(rhs)) {
            self.emit_compare_operand(lhs)?;
            self.emit_compare_operand(rhs)?;
            self.b.emit(Op::LoadInt(i64::from(op == BinOp::Ne)), 0);
            self.b.emit(Op::CallBuiltin(host::GIFACE_EQ, 3), 0);
            return Ok(());
        }

        // Comparisons pick string vs numeric ops from the operand types.
        if let Some(strcmp) = str_compare_op(op) {
            let is_str = self.infer(lhs) == NumType::Str || self.infer(rhs) == NumType::Str;
            // Go compares at one type: an untyped constant beside a `float32`
            // becomes a `float32`, so `float32(0.1) == 0.1` is true where
            // comparing the `f32` against the `f64` 0.1 would be false. Only one
            // side can be untyped in a legal program, so rounding both is safe.
            let f32ish = self.is_f32(lhs) || self.is_f32(rhs);
            // An ordered comparison of unsigned 64-bit operands reads the sign
            // bit, so it goes through the unsigned builtin. `==`/`!=` do not —
            // equal bit patterns are equal at either signedness.
            let u64ish = !is_str && (self.is_u64(lhs) || self.is_u64(rhs));
            self.emit_compare_operand(lhs)?;
            self.emit_f32_round(f32ish && !self.is_f32(lhs));
            self.emit_compare_operand(rhs)?;
            self.emit_f32_round(f32ish && !self.is_f32(rhs));
            if self.emit_u64_arith(op, u64ish, 0) {
                return Ok(());
            }
            self.b
                .emit(if is_str { strcmp } else { num_compare_op(op) }, 0);
            return Ok(());
        }

        // Arithmetic.
        self.check_single_value(lhs, 0)?;
        self.check_single_value(rhs, 0)?;
        let l = self.infer(lhs);
        let r = self.infer(rhs);
        let f32ish = self.is_f32(lhs) || self.is_f32(rhs);
        self.expr(lhs)?;
        self.expr(rhs)?;
        if self.emit_f32_arith(op, f32ish, 0) {
            return Ok(());
        }
        // `/`, `%` and `>>` are the arithmetic operators whose result depends on
        // the sign bit; at an unsigned 64-bit type they take the unsigned form.
        // A shift's type is the left operand's alone — `int8 >> uint` is a
        // signed (arithmetic) shift however the *count* is typed.
        let u64ish = match op {
            BinOp::Shl | BinOp::Shr => self.is_u64(lhs),
            _ => self.is_u64(lhs) || self.is_u64(rhs),
        };
        if self.emit_u64_arith(op, u64ish, 0) {
            return Ok(());
        }
        self.emit_arith(op, l, r, is_nonzero_const(rhs), 0);
        // Go's arithmetic is fixed-width: a sized operand makes the result wrap
        // at its own width, not at 64 bits.
        if let Some(ty) = self.sized_int_ty(&Expr::Binary {
            op,
            lhs: Box::new(lhs.clone()),
            rhs: Box::new(rhs.clone()),
        }) {
            self.emit_narrow(&ty, 0);
        }
        Ok(())
    }

    /// Whether `e`'s static Go type is an interface — `any`/`interface{}`,
    /// `error`, or an interface the program declares.
    ///
    /// This is what makes the dynamic-type rule reachable without putting a
    /// builtin call on every `==`: a comparison of two concrete types is decided
    /// by the type checker, so the native op is already right for it.
    fn is_iface(&self, e: &Expr) -> bool {
        let ty = base_type(&self.type_name(e));
        matches!(ty.as_str(), "interface{}" | "interface{ }") || self.iface_names.contains(&ty)
    }

    /// Whether `e`'s static Go type is `float32`.
    ///
    /// It is the one float width whose arithmetic differs from the `f64` the
    /// value model holds, and — unlike the narrow integer types — it cannot be
    /// fixed by rounding the `f64` result afterwards, because that rounds twice.
    /// So the whole operation has to be done in `f32`, which is what this
    /// predicate selects. Go's untyped constants take the other operand's type,
    /// so one `float32` operand anywhere makes the expression `float32`.
    fn is_f32(&self, e: &Expr) -> bool {
        if self.type_name(e) == "float32" {
            return true;
        }
        match e {
            Expr::Unary { op: UnOp::Neg, rhs } => self.is_f32(rhs),
            Expr::Binary { op, lhs, rhs } if str_compare_op(*op).is_none() => {
                self.is_f32(lhs) || self.is_f32(rhs)
            }
            // The conversion `float32(x)` names its own type.
            Expr::Call { func, args, .. } => {
                args.len() == 1 && matches!(func.as_ref(), Expr::Ident(n) if n == "float32")
            }
            Expr::Index { recv, .. } => self.elem_ty_of(recv).as_deref() == Some("float32"),
            _ => false,
        }
    }

    /// The unsigned 64-bit Go type `e` statically has (`uint64`, `uint` or
    /// `uintptr`), or `None`.
    ///
    /// These three share `Value::Int`'s 64-bit two's-complement bit pattern, so
    /// unlike the narrow widths they need no wrapping. What they need is the
    /// operations that read the sign bit — `/`, `%`, `>>`, the ordered
    /// comparisons, the conversion to a float, and display — done unsigned.
    /// The traversal mirrors [`Self::sized_int_ty`]: Go's untyped constants take
    /// the type of the other operand, so one unsigned operand fixes the whole
    /// expression, and a shift takes its type from the left operand alone.
    fn u64_ty(&self, e: &Expr) -> Option<String> {
        let named = |t: &str| is_uint64_ty(t).then(|| t.to_string());
        if let Some(t) = named(&base_type(&self.type_name(e))) {
            return Some(t);
        }
        match e {
            Expr::Ident(n) => self.decl_types.get(n).and_then(|t| named(&base_type(t))),
            Expr::Unary {
                op: UnOp::Neg | UnOp::BitNot,
                rhs,
            } => self.u64_ty(rhs),
            Expr::Binary {
                op: BinOp::Shl | BinOp::Shr,
                lhs,
                ..
            } => self.u64_ty(lhs),
            Expr::Binary { op, lhs, rhs } if str_compare_op(*op).is_none() => {
                self.u64_ty(lhs).or_else(|| self.u64_ty(rhs))
            }
            Expr::Call { func, .. } => match func.as_ref() {
                // A conversion `uint64(x)` names its own type.
                Expr::Ident(n) if is_uint64_ty(n) => Some(n.clone()),
                Expr::Ident(n) => self
                    .funcs
                    .get(n)
                    .and_then(|s| named(&base_type(&s.result_ty))),
                _ => None,
            },
            // A slice/map element takes its type from the container's.
            Expr::Index { recv, .. } => self.elem_ty_of(recv).and_then(|t| named(&base_type(&t))),
            _ => None,
        }
    }

    /// The element type of a channel-valued expression, or `""` when go-rs never
    /// recorded one (a channel reached through an untyped binding). The zero of
    /// an unknown type is `emit_zero`'s default, which is what an untyped
    /// receive would have produced anyway.
    fn chan_elem_ty(&self, e: &Expr) -> String {
        let t = self.type_name(e);
        t.strip_prefix("chan ").unwrap_or_default().to_string()
    }

    /// Emit the zero value of a channel's element type. Unlike [`Self::emit_zero`]
    /// this also builds a struct type's zero (all fields at their own zero),
    /// which is what a receive from a closed `chan T` must yield for a struct `T`.
    fn emit_elem_zero(&mut self, ty: &str, line: u32) -> Result<(), String> {
        if self.structs.contains(ty) {
            return self.struct_lit(ty, &[]);
        }
        self.emit_zero(ty, line);
        Ok(())
    }

    /// Emit one channel receive: `Op::ChanRecv`, then map the drained-closed
    /// sentinel to the element type's zero so it never reaches a Go value.
    /// The received value is left on the stack.
    fn emit_recv(&mut self, chan: &Expr, line: u32) -> Result<(), String> {
        let elem = self.chan_elem_ty(chan);
        self.expr(chan)?;
        self.b.emit(Op::ChanRecv, line);
        self.emit_elem_zero(&elem, line)?;
        self.b.emit(Op::CallBuiltin(host::GCHAN_VAL, 2), line);
        Ok(())
    }

    /// Whether `e`'s static Go type is one of the unsigned 64-bit types.
    fn is_u64(&self, e: &Expr) -> bool {
        self.u64_ty(e).is_some()
    }

    /// How a `fmt` argument's unsigned 64-bit integers should be tagged, read
    /// exactly as [`Self::f32_box_spec`] reads its own: `Some(("", ty))` tags the
    /// value or each element of a container, `Some(("x,y", ty))` names the
    /// unsigned fields of a struct operand.
    fn u64_box_spec(&self, e: &Expr) -> Option<(String, String)> {
        if let Some(ty) = self.u64_ty(e) {
            return Some((String::new(), ty));
        }
        let elem = self
            .elem_ty_of(e)
            .map(|t| base_type(&t).to_string())
            .unwrap_or_else(|| base_type(&self.type_name(e)).to_string());
        if is_uint64_ty(&elem) {
            return Some((String::new(), elem));
        }
        let fields = self.struct_fields.get(&elem)?;
        let unsigned: Vec<&(String, String)> = fields
            .iter()
            .filter(|(_, ft)| is_uint64_ty(&base_type(ft)))
            .collect();
        let ty = unsigned.first()?.1.clone();
        let spec = unsigned
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>()
            .join(",");
        Some((spec, base_type(&ty).to_string()))
    }

    /// Emit `op` as a single unsigned 64-bit operation when the expression is
    /// statically unsigned and the operator reads the sign bit, and report
    /// whether it did (the caller emits the ordinary native op otherwise).
    /// `+ - * << & | ^ &^` are sign-agnostic in two's complement and never come
    /// through here.
    fn emit_u64_arith(&mut self, op: BinOp, is_u64: bool, line: u32) -> bool {
        let Some(code) = u64_op_code(op).filter(|_| is_u64) else {
            return false;
        };
        self.b.emit(Op::LoadInt(code), line);
        self.b.emit(Op::CallBuiltin(host::GU64_ARITH, 3), line);
        if matches!(op, BinOp::Div | BinOp::Mod) {
            self.emit_panic_check(line);
        }
        true
    }

    /// The defined type a `fmt` argument should be tagged with, or `None` when
    /// its static type is not one. A `*T` is tagged with `T`: go-rs holds a
    /// pointer and its pointee as the same handle, and `%T` of a `*Weekday`
    /// naming `main.Weekday` is nearer than naming `int`.
    fn named_box_spec(&self, e: &Expr) -> Option<String> {
        // Arithmetic on a defined type keeps it — `Weekday(3) + 1` is a
        // `Weekday` — so a binary operator reports whichever side names one.
        if let Expr::Binary { lhs, rhs, .. } = e {
            return self
                .named_box_spec(lhs)
                .or_else(|| self.named_box_spec(rhs));
        }
        // `-Weekday(3)` and `^n` keep the operand's type the same way.
        if let Expr::Unary {
            op: UnOp::Neg | UnOp::BitNot,
            rhs,
        } = e
        {
            return self.named_box_spec(rhs);
        }
        // A method's declared result type, which no other static-type path
        // reaches: `Weekday(3).next()` is a `Weekday`.
        if let Expr::Call { func, .. } = e {
            if let Expr::Selector { recv, field } = func.as_ref() {
                let key = (self.type_name(recv), field.clone());
                if let Some(rt) = self.method_result_ty.get(&key) {
                    let rt = rt.trim_start_matches('*');
                    if self.defined_types.contains_key(rt) {
                        return Some(rt.to_string());
                    }
                }
            }
        }
        let ty = self.type_name(e);
        let ty = ty.trim_start_matches('*');
        if self.defined_types.contains_key(ty) {
            return Some(ty.to_string());
        }
        // A container whose written type *mentions* a defined type is named by
        // it too — `map[myStr]myInt` prints as `map[main.myStr]main.myInt`. Only
        // the map case needs this: a slice already carries its element type
        // through [`Self::elem_tag_spec`].
        let mentions_defined = ty
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .any(|w| self.defined_types.contains_key(w));
        (mentions_defined && ty.starts_with("map[")).then(|| ty.to_string())
    }

    /// The sized integer type a `fmt` argument should be tagged with so `%T`
    /// names its width, or `None` when its static type is not one.
    ///
    /// `int8`, `int16`, `int32`/`rune`, `int64`, `uint8`/`byte`, `uint16` and
    /// `uint32` are all a plain `Value::Int` at run time, so without the tag
    /// every one of them answers `%T` with `int`. The traversal mirrors
    /// [`Self::u64_ty`] — the same expression forms carry a width — and the tag
    /// is applied only where [`Self::named_box_spec`] found no defined type,
    /// because a `type myByte byte` is `main.myByte` to `%T`, not `uint8`.
    fn sized_int_box_spec(&self, e: &Expr) -> Option<String> {
        let named = |t: &str| is_sized_int_ty(t).then(|| t.to_string());
        let ty = base_type(&self.type_name(e));
        if let Some(t) = named(&ty) {
            return Some(t);
        }
        // A map object stores no written type — `%T` describes it from the
        // key/value pairs it holds, which cannot tell a `uint8` value from an
        // `int` one. So a map whose written type mentions a sized integer
        // carries that type in whole, exactly as a map mentioning a defined type
        // does in [`Self::named_box_spec`]. A slice needs none of this: it
        // already carries its element type through [`Self::elem_tag_spec`].
        if ty.starts_with("map[")
            && ty
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(is_sized_int_ty)
        {
            return Some(ty);
        }
        match e {
            Expr::Ident(n) => self.decl_types.get(n).and_then(|t| named(&base_type(t))),
            // A width survives negation and complement.
            Expr::Unary {
                op: UnOp::Neg | UnOp::BitNot,
                rhs,
            } => self.sized_int_box_spec(rhs),
            // A shift takes its type from the shifted operand, never the count.
            Expr::Binary {
                op: BinOp::Shl | BinOp::Shr,
                lhs,
                ..
            } => self.sized_int_box_spec(lhs),
            // Any other arithmetic operator requires both operands to already
            // have the same type, so whichever side names a width names the
            // result's. Comparisons yield a `bool` and are excluded.
            Expr::Binary { op, lhs, rhs }
                if !matches!(
                    op,
                    BinOp::Eq
                        | BinOp::Ne
                        | BinOp::Lt
                        | BinOp::Gt
                        | BinOp::Le
                        | BinOp::Ge
                        | BinOp::And
                        | BinOp::Or
                ) =>
            {
                self.sized_int_box_spec(lhs)
                    .or_else(|| self.sized_int_box_spec(rhs))
            }
            Expr::Call { func, .. } => match func.as_ref() {
                // A conversion `int8(x)` names its own type.
                Expr::Ident(n) if is_sized_int_ty(n) => Some(n.clone()),
                Expr::Ident(n) => self
                    .funcs
                    .get(n)
                    .and_then(|s| named(&base_type(&s.result_ty))),
                _ => None,
            },
            // A slice/map element takes its type from the container's.
            Expr::Index { recv, .. } => self.elem_ty_of(recv).and_then(|t| named(&base_type(&t))),
            _ => None,
        }
    }

    /// The written type a `fmt` argument's slices should be tagged with, or
    /// `None` when the operand is not a container the compiler has a static type
    /// for. Only containers need the tag: it exists so the formatter can tell a
    /// `[]byte` (text under `%s`/`%q`/`%x`) from a `[]int` (which those verbs
    /// distribute over), and an operand with no static type is left untagged,
    /// where the guess from the element values stands in.
    fn elem_tag_spec(&self, e: &Expr) -> Option<String> {
        let ty = self.type_name(e);
        let ty = ty.trim_start_matches('*');
        (ty.starts_with("[]") || ty.starts_with("map[") || array_elem_ty(ty).is_some())
            .then(|| ty.to_string())
    }

    /// How a `fmt` argument's `float32`s should be tagged, or `None` when it has
    /// none. `Some("")` tags the value (or each element of a `[]float32` /
    /// `map[K]float32`); `Some("x,y")` names the `float32` fields of a struct
    /// operand, which `fmt` renders inline and so must tag one level down.
    fn f32_box_spec(&self, e: &Expr) -> Option<String> {
        if self.is_f32(e) {
            return Some(String::new());
        }
        // A container operand is tagged through its element type; anything else
        // through its own.
        let elem = self
            .elem_ty_of(e)
            .unwrap_or_else(|| base_type(&self.type_name(e)));
        if elem == "float32" {
            return Some(String::new());
        }
        let spec = self
            .struct_fields
            .get(&elem)?
            .iter()
            .filter(|(_, ft)| base_type(ft) == "float32")
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>()
            .join(",");
        (!spec.is_empty()).then_some(spec)
    }

    /// Round the value on the stack to 32-bit float width, when `apply`.
    fn emit_f32_round(&mut self, apply: bool) {
        if !apply {
            return;
        }
        let c = self.b.add_constant(Value::str("float32"));
        self.b.emit(Op::LoadConst(c), 0);
        self.b.emit(Op::CallBuiltin(host::GCONV, 2), 0);
    }

    /// Emit `op` as a single 32-bit-wide float operation when either operand is
    /// statically `float32`, and report whether it did (the caller emits the
    /// ordinary native arithmetic otherwise).
    fn emit_f32_arith(&mut self, op: BinOp, is_f32: bool, line: u32) -> bool {
        let Some(code) = f32_op_code(op).filter(|_| is_f32) else {
            return false;
        };
        self.b.emit(Op::LoadInt(code), line);
        self.b.emit(Op::CallBuiltin(host::GF32_ARITH, 3), line);
        true
    }

    /// Wrap the integer on the stack to `ty`'s declared width — Go's fixed-width
    /// arithmetic, where `int8(127) + 1` is `-128` rather than `128`.
    ///
    /// Signed: shift left to put the type's sign bit in bit 63, then shift
    /// arithmetically back, which discards the high bits and sign-extends.
    /// Unsigned: mask the high bits off. Both are ordinary ops the interpreter,
    /// the tracing JIT and the AOT backend all lower natively, and both are
    /// emitted per site, so widths mix freely inside one chunk. Emits nothing for
    /// a 64-bit type, whose wrapping `Value::Int` already is.
    fn emit_narrow(&mut self, ty: &str, line: u32) {
        let Some((bits, signed)) = int_width(ty) else {
            return;
        };
        if signed {
            let shift = 64 - bits as i64;
            self.b.emit(Op::LoadInt(shift), line);
            self.b.emit(Op::Shl, line);
            self.b.emit(Op::LoadInt(shift), line);
            self.b.emit(Op::Shr, line);
        } else {
            self.b.emit(Op::LoadInt((1i64 << bits) - 1), line);
            self.b.emit(Op::BitAnd, line);
        }
    }

    /// The narrower-than-64-bit integer type an arithmetic expression produces,
    /// if any. Go's untyped constants take the type of the other operand, so one
    /// sized operand anywhere fixes the whole expression's width; a shift takes
    /// its type from the left operand alone (the count has its own).
    fn sized_int_ty(&self, e: &Expr) -> Option<String> {
        match e {
            Expr::Ident(n) => self
                .decl_types
                .get(n)
                .filter(|t| int_width(t).is_some())
                .cloned(),
            Expr::Unary {
                op: UnOp::Neg | UnOp::BitNot,
                rhs,
            } => self.sized_int_ty(rhs),
            Expr::Binary {
                op: BinOp::Shl | BinOp::Shr,
                lhs,
                ..
            } => self.sized_int_ty(lhs),
            Expr::Binary { op, lhs, rhs } if str_compare_op(*op).is_none() => {
                self.sized_int_ty(lhs).or_else(|| self.sized_int_ty(rhs))
            }
            Expr::Call { func, .. } => match func.as_ref() {
                // A conversion `int8(x)` names its own type.
                Expr::Ident(n) if int_width(n).is_some() => Some(n.clone()),
                Expr::Ident(n) => self
                    .funcs
                    .get(n)
                    .map(|s| base_type(&s.result_ty))
                    .filter(|t| int_width(t).is_some()),
                _ => None,
            },
            Expr::Selector { .. } | Expr::TypeAssert { .. } => {
                let t = self.type_name(e);
                (int_width(&t).is_some()).then_some(t)
            }
            // A slice element takes its width from the slice's element type.
            Expr::Index { recv, .. } => self.elem_ty_of(recv).filter(|t| int_width(t).is_some()),
            _ => None,
        }
    }

    /// The written element type of a slice-valued expression, when go-rs recorded
    /// one — how `xs[i] += n` learns the width to wrap at.
    fn elem_ty_of(&self, e: &Expr) -> Option<String> {
        match e {
            Expr::Ident(n) => self.decl_types.get(n).and_then(|t| elem_of_type(t)),
            Expr::SliceLit { elem_ty, .. } => Some(elem_ty.clone()),
            Expr::MapLit { val_ty, .. } => Some(val_ty.clone()),
            Expr::Make {
                is_map, elem_ty, ..
            } => {
                if *is_map {
                    elem_of_type(elem_ty)
                } else {
                    Some(elem_ty.clone())
                }
            }
            _ => None,
        }
    }

    /// Emit an arithmetic op for two already-pushed operands, appending
    /// `TruncInt` for integer division (Go truncates `int / int` toward zero).
    /// `safe_div` is true when the divisor is a provably-nonzero constant, so
    /// integer `/`/`%` can use the native ops the JIT/AOT keep in registers.
    fn emit_arith(&mut self, op: BinOp, l: NumType, r: NumType, safe_div: bool, line: u32) {
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
            // `%` is integer-only in Go. With a provably-nonzero constant divisor
            // (`i % 7`), emit the native `Op::Mod` — an integer op the JIT and AOT
            // lower to a register `srem`, keeping a hot loop entirely in registers
            // (~2 orders of magnitude faster). Otherwise route through GIMOD so a
            // zero divisor raises a panic (recoverable under `recover`, else it
            // aborts like Go).
            BinOp::Mod => {
                if safe_div {
                    self.b.emit(Op::Mod, line);
                } else {
                    self.b.emit(Op::CallBuiltin(host::GIMOD, 2), line);
                    self.emit_panic_check(line);
                }
            }
            BinOp::Div => {
                if l == NumType::Int && r == NumType::Int {
                    if safe_div {
                        // Native float divide + truncate toward zero — both scalar
                        // ops the JIT/AOT keep in registers (divisor is nonzero).
                        self.b.emit(Op::Div, line);
                        self.b.emit(Op::TruncInt, line);
                    } else {
                        // Truncating integer division that panics on a zero
                        // divisor (recoverable / aborting to match Go).
                        self.b.emit(Op::CallBuiltin(host::GIDIV, 2), line);
                        self.emit_panic_check(line);
                    }
                } else if l == NumType::Float || r == NumType::Float {
                    // Float division: `x / 0.0` yields ±Inf like Go, no panic.
                    // fusevm's `Op::Div` returns `Undef` for a zero divisor, so
                    // only a provably-nonzero constant divisor can use it (and
                    // keep the JIT/AOT register lowering); everything else goes
                    // through the IEEE builtin.
                    if safe_div {
                        self.b.emit(Op::Div, line);
                    } else {
                        self.b.emit(Op::CallBuiltin(host::GFDIV, 2), line);
                    }
                } else {
                    // At least one operand's numeric category is unknown at
                    // compile time (indexing a slice/map, a method result, an
                    // `interface{}` value). Go picks integer vs float division
                    // from the static types; go-rs defers the same choice to the
                    // runtime representations rather than assuming float, which
                    // made `xs[0] / 2` yield `3.5` instead of `3`.
                    self.b.emit(Op::CallBuiltin(host::GDYNDIV, 2), line);
                    self.emit_panic_check(line);
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
                    // `fmt.Errorf(f, args...)` builds a real error value whose
                    // message is `fmt.Sprintf(f, args...)`. Which value depends on
                    // how many operands the format binds to `%w`, exactly as Go's
                    // `fmt.Errorf` does: none gives a plain `errors.New`-shaped
                    // error, one a `*wrapError` (`Unwrap() error`), more a
                    // `*wrapErrors` (`Unwrap() []error`). The linker synthesizes
                    // all three types when Errorf is used.
                    if field == "Errorf" {
                        let msg = Expr::Call {
                            func: Box::new(Expr::Selector {
                                recv: Box::new(Expr::Ident("fmt".to_string())),
                                field: "Sprintf".to_string(),
                            }),
                            args: args.to_vec(),
                            // `fmt.Errorf(f, a...)` spreads into the message the
                            // same way `Printf` does — the synthesized `Sprintf`
                            // has to carry the flag or the slice formats as one
                            // operand.
                            spread,
                            line,
                        };
                        let wrapped: Vec<Expr> = match args.first() {
                            Some(Expr::Str(f)) => wrap_operands(f)
                                .into_iter()
                                .filter_map(|i| args.get(i + 1).cloned())
                                .collect(),
                            // A non-literal format cannot be scanned for `%w`, so
                            // the result wraps nothing (Go would decide at run
                            // time).
                            _ => Vec::new(),
                        };
                        let mut fields = vec![(Some("s".to_string()), msg)];
                        let type_name = match wrapped.len() {
                            0 => "$errorString",
                            1 => {
                                fields.push((
                                    Some("err".to_string()),
                                    wrapped.into_iter().next().expect("one wrapped error"),
                                ));
                                "$wrapError"
                            }
                            _ => {
                                fields.push((
                                    Some("errs".to_string()),
                                    Expr::SliceLit {
                                        elem_ty: "error".to_string(),
                                        elems: wrapped,
                                        array_len: None,
                                    },
                                ));
                                "$wrapErrors"
                            }
                        };
                        let addr = Expr::Unary {
                            op: UnOp::Addr,
                            rhs: Box::new(Expr::StructLit {
                                type_name: type_name.to_string(),
                                fields,
                            }),
                        };
                        return self.expr(&addr);
                    }
                    // `fmt.Fprint*(w, …)` is `w.Write([]byte(fmt.Sprint*(…)))`
                    // and nothing else. Both halves already exist — the
                    // formatting one as `Sprint*`, the writing one as an
                    // ordinary method call on whatever the program handed in —
                    // so writer-directed output needs no host support at all,
                    // only the rewrite. The result is `Write`'s own
                    // `(n int, err error)`, which is what Go's returns.
                    if let Some(sprint) = match field.as_str() {
                        "Fprintf" => Some("Sprintf"),
                        "Fprint" => Some("Sprint"),
                        "Fprintln" => Some("Sprintln"),
                        _ => None,
                    } {
                        let Some((w, rest)) = args.split_first() else {
                            return Err(format!(
                                "go-rs: `fmt.{field}` needs a writer (line {line})"
                            ));
                        };
                        let text = Expr::Call {
                            func: Box::new(Expr::Selector {
                                recv: Box::new(Expr::Ident("fmt".to_string())),
                                field: sprint.to_string(),
                            }),
                            args: rest.to_vec(),
                            spread,
                            line,
                        };
                        let bytes = Expr::Call {
                            func: Box::new(Expr::Ident("[]byte".to_string())),
                            args: vec![text],
                            spread: false,
                            line,
                        };
                        return self.call(
                            &Expr::Selector {
                                recv: Box::new(w.clone()),
                                field: "Write".to_string(),
                            },
                            &[bytes],
                            false,
                            line,
                        );
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
                    // `fmt.Printf(f, xs...)` — the last argument is a slice
                    // standing for the operands it holds, not an operand of its
                    // own. Its length is a run-time fact and `CallBuiltin`'s
                    // arity is a compile-time one, so it rides in under
                    // `GSPREAD` and `pop_args` expands it. The per-argument type
                    // tags below are skipped for it: they describe one written
                    // static type, and a spread slice is `[]any` whose elements
                    // each carry their own.
                    let last = args.len().saturating_sub(1);
                    for (n, a) in args.iter().enumerate() {
                        if spread && n == last {
                            self.expr(a)?;
                            self.b.emit(Op::CallBuiltin(host::GSPREAD, 1), line);
                            continue;
                        }
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
                        // `fmt` renders a float at its own width's precision, so
                        // a statically-`float32` operand carries that width in.
                        // This is the only place the tag is applied — it never
                        // reaches arithmetic, where it would be a heap value.
                        if let Some(spec) = self.f32_box_spec(a) {
                            let c = self.b.add_constant(Value::str(spec));
                            self.b.emit(Op::LoadConst(c), line);
                            self.b.emit(Op::CallBuiltin(host::GF32_BOX, 2), line);
                        }
                        // Likewise for width's other half: an unsigned 64-bit
                        // operand holds the right bits but reads negative, so it
                        // carries its signedness in the same way.
                        if let Some((spec, ty)) = self.u64_box_spec(a) {
                            let c = self.b.add_constant(Value::str(spec));
                            self.b.emit(Op::LoadConst(c), line);
                            let t = self.b.add_constant(Value::str(ty));
                            self.b.emit(Op::LoadConst(t), line);
                            self.b.emit(Op::CallBuiltin(host::GU64_BOX, 3), line);
                        }
                        // `%s`/`%q`/`%x` read a `[]byte` as text and distribute
                        // over the elements of every other slice, and the values
                        // alone cannot tell the two apart — so a container
                        // operand carries its written type in.
                        if let Some(ty) = self.elem_tag_spec(a) {
                            let t = self.b.add_constant(Value::str(ty));
                            self.b.emit(Op::LoadConst(t), line);
                            self.b.emit(Op::CallBuiltin(host::GELEM_TAG, 2), line);
                        }
                        // A defined type is its base at run time, so its name
                        // exists only in the static type — and `%T` prints it.
                        // A sized integer is the same problem one level down:
                        // `int8` and `uint16` are also a bare `Value::Int`, so
                        // when no defined type claims the operand its *width*
                        // carries the name instead.
                        if let Some(ty) = self
                            .named_box_spec(a)
                            .or_else(|| self.sized_int_box_spec(a))
                        {
                            let t = self.b.add_constant(Value::str(ty));
                            self.b.emit(Op::LoadConst(t), line);
                            self.b.emit(Op::CallBuiltin(host::GNAMED_BOX, 2), line);
                        }
                    }
                    let argc = Self::call_arity(args.len(), &format!("fmt.{field}"), line)?;
                    self.b.emit(Op::CallBuiltin(id, argc), line);
                    return Ok(());
                }
                // Standard-library package calls.
                // `sort.Slice(s, less)` / `sort.SliceStable(s, less)` — the
                // comparator is a VM closure a host builtin can't call, so lower
                // to the linker-synthesized `$sortSlice` (an in-language insertion
                // sort that calls `less`).
                if pkg == "sort" && (field == "Slice" || field == "SliceStable") {
                    return self.call(&Expr::Ident("$sortSlice".to_string()), args, false, line);
                }
                if matches!(pkg.as_str(), "strings" | "strconv" | "math" | "sort" | "os") {
                    let id = host::stdlib::resolve(pkg, field).ok_or_else(|| {
                        format!("go-rs: unsupported call `{pkg}.{field}` (line {line})")
                    })?;
                    for a in args {
                        self.expr(a)?;
                    }
                    let argc = Self::call_arity(args.len(), &format!("{pkg}.{field}"), line)?;
                    self.b.emit(Op::CallBuiltin(id, argc), line);
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
                // `float64(u)` on an unsigned 64-bit operand widens the
                // *unsigned* value: `float64(uint64(1)<<63)` is 9.22e+18, where
                // reading the same bits as an `i64` would give -9.22e+18.
                if matches!(name.as_str(), "float32" | "float64") && self.is_u64(&args[0]) {
                    self.b.emit(Op::LoadInt(0), line);
                    self.b.emit(Op::LoadInt(host::u64_op::TOFLOAT), line);
                    self.b.emit(Op::CallBuiltin(host::GU64_ARITH, 3), line);
                    if name == "float32" {
                        self.emit_f32_round(true);
                    }
                    return Ok(());
                }
                let c = self.b.add_constant(Value::str(name.clone()));
                self.b.emit(Op::LoadConst(c), line);
                self.b.emit(Op::CallBuiltin(host::GCONV, 2), line);
                return Ok(());
            }
            // A conversion to a defined type — `Weekday(3)`, `mySlice{…}` (which
            // the parser writes as `mySlice([]int{…})`). The defined type shares
            // its base's representation, so the value is the base's conversion:
            // a predeclared base narrows or reformats through `GCONV`, and any
            // other base — a slice, map, func, chan or pointer — is the
            // identity. Only the *name* is new, and it is carried at a `fmt`
            // argument position by [`Self::named_box_spec`].
            if args.len() == 1 {
                if let Some(base) = self.defined_types.get(name).cloned() {
                    self.expr(&args[0])?;
                    if is_conversion_type(&base) {
                        let c = self.b.add_constant(Value::str(base));
                        self.b.emit(Op::LoadConst(c), line);
                        self.b.emit(Op::CallBuiltin(host::GCONV, 2), line);
                    }
                    return Ok(());
                }
            }
            // `panic(v)` records the panic then unwinds to the function's defer
            // drain (jump patched to the panic epilogue).
            if name == "panic" {
                for a in args {
                    self.emit_value(a)?;
                }
                let argc = Self::call_arity(args.len(), "panic", line)?;
                self.b.emit(Op::CallBuiltin(host::GPANIC, argc), line);
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
                // The element type when it is a Go *value* type (a struct or a
                // fixed-size array), which `append` copies — empty otherwise.
                // Read from the static type here because the runtime cannot tell
                // a `[]T` element from a `[]*T` one, nor a `[][2]int` element
                // from a `[][]int` one.
                let elem = args
                    .last()
                    .map(|a| self.elem_type_of(&self.type_name(a)))
                    .unwrap_or_default();
                let copy_ty = if self.structs.contains(&elem) || array_elem_ty(&elem).is_some() {
                    elem
                } else {
                    String::new()
                };
                let c = self.b.add_constant(Value::str(copy_ty));
                self.b.emit(Op::LoadConst(c), line);
                for a in args {
                    self.expr(a)?;
                }
                let argc = Self::call_arity(args.len() + 1, "append", line)?;
                self.b
                    .emit(Op::CallBuiltin(host::GAPPEND_SPREAD, argc), line);
                return Ok(());
            }
            // `errors.As(err, &target)` — Go recovers the target's type from the
            // pointer at run time with reflectlite. go-rs already knows it
            // statically, so the call lowers to the vendored package's `asTag`
            // walk keyed on that type's runtime tag, and assigns the target only
            // when the walk finds a match (as Go's `As` does).
            if name == "errors.As" {
                return self.errors_as(args, line);
            }
            // The vendored `errors` package's one host intrinsic: the runtime type
            // tag a type switch dispatches on, which `asTag` compares against.
            if name == "errors.runtimeTypeTag" && args.len() == 1 {
                self.expr(&args[0])?;
                self.b.emit(Op::CallBuiltin(host::GTYPETAG, 1), line);
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
                for (i, a) in args.iter().enumerate() {
                    // `append(s, v)` stores a *copy* of a struct element — the
                    // appended variable stays independent of the slice. The base
                    // slice (argument 0) is a reference and passes through; every
                    // other builtin here takes no struct by value, so the copy is
                    // asked for only where Go performs one.
                    if id == host::GAPPEND && i > 0 {
                        // The appended element takes the slice's element type,
                        // so `append(s, 1)` on a `[]float64` appends a float.
                        let elem = self.elem_type_of(&self.type_name(&args[0]));
                        self.emit_typed(a, &elem)?;
                    } else if id == host::GDELETE && i == 1 {
                        // `delete(m, 1)` on a `map[float64]V` looks the key up
                        // the same way `m[1]` does, so it converts the same way.
                        self.emit_map_key(&args[0], a)?;
                    } else {
                        self.expr(a)?;
                    }
                }
                let argc = Self::call_arity(args.len(), name, line)?;
                self.b.emit(Op::CallBuiltin(id, argc), line);
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
                let param_tys = sig.param_tys.clone();
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
                    for (i, a) in args[..fixed].iter().enumerate() {
                        self.emit_arg(a, param_tys.get(i))?;
                    }
                    if spread {
                        // `f(a, xs...)` — the last argument is the slice itself.
                        self.emit_value(&args[fixed])?;
                    } else {
                        // Pack the remaining arguments into a fresh slice.
                        let rest = args[fixed..].to_vec();
                        self.emit_lit_chunked(host::GSLICE_LIT, 0, 1, rest.len(), line, |c, i| {
                            c.emit_value(&rest[i])
                        })?;
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
                for (i, a) in args.iter().enumerate() {
                    self.emit_arg(a, param_tys.get(i))?;
                }
                let idx = self.b.add_name(name);
                self.b.emit(Op::Call(idx, args.len() as u8), line);
                self.emit_panic_check(line);
                return Ok(());
            }
            // A conversion to an interface type — `error(e)`, `any(3)` — is the
            // identity: the dynamic value is unchanged, only its static type is.
            if args.len() == 1 && self.iface_names.contains(name) {
                return self.expr(&args[0]);
            }
            // With an inline `rust {}` block present, an otherwise-unresolved
            // bare name may be an FFI export — dispatch it by name at runtime.
            if self.has_ffi {
                for a in args {
                    self.expr(a)?;
                }
                let c = self.b.add_constant(Value::str(name.clone()));
                self.b.emit(Op::LoadConst(c), line);
                let argc = Self::call_arity(args.len() + 1, name, line)?;
                self.b.emit(Op::CallBuiltin(host::GFFI_CALL, argc), line);
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

/// How Go names an interface type in an `interface conversion` panic: a
/// program-declared one is package-qualified (`main.St`), the predeclared
/// `error` is not, and an anonymous one is spelled out by its method set.
fn iface_display(ty: &str) -> String {
    let name = base_type(ty);
    if name == "error" || name.starts_with("interface{") {
        name
    } else {
        format!("main.{name}")
    }
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

/// The operand indices (0-based among `fmt.Errorf`'s arguments after the format)
/// that a literal `format` binds to a `%w` verb — what Go's `fmt.Errorf` records
/// as the errors it wraps.
///
/// Walks the same shape `fmt` does: `%`, flags, width, `.` precision, verb. `%%`
/// consumes no operand; a `*` width or precision consumes one of its own.
fn wrap_operands(format: &str) -> Vec<usize> {
    let chars: Vec<char> = format.chars().collect();
    let mut out = Vec::new();
    let mut arg = 0usize;
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '%' {
            i += 1;
            continue;
        }
        i += 1;
        while i < chars.len() && matches!(chars[i], '-' | '+' | '#' | ' ' | '0') {
            i += 1;
        }
        for _ in 0..2 {
            if i < chars.len() && chars[i] == '*' {
                arg += 1;
                i += 1;
            } else {
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            // Only a `.` introduces the second (precision) round.
            if !(i < chars.len() && chars[i] == '.') {
                break;
            }
            i += 1;
        }
        let Some(&verb) = chars.get(i) else { break };
        i += 1;
        if verb == '%' {
            continue;
        }
        if verb == 'w' {
            out.push(arg);
        }
        arg += 1;
    }
    out
}

/// Normalize a written type to the runtime tag [`host::GTYPETAG`] produces:
/// pointers/named types → the base name, `[]T` → `[]`, `map[..]` → `map`,
/// `func…` → `func`, and interface types (`any`, `interface{…}`, `error`) → `""`
/// (which matches any value).
fn type_to_tag(ty: &str) -> String {
    let ty = ty.trim_start_matches('*');
    // A fixed-size array shares the slice's runtime object, so it shares the
    // runtime tag: `[3]int` and `[]int` both answer `[]`.
    if ty.starts_with("[]") || array_elem_ty(ty).is_some() {
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

/// Whether `e` is a provably-nonzero constant — so an integer `/`/`%` by it can
/// never divide by zero and may use the native (register-lowered) op instead of
/// the panic-checking builtin.
fn is_nonzero_const(e: &Expr) -> bool {
    matches!(e, Expr::Int(n) if *n != 0)
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
        | Stmt::Break(line, _)
        | Stmt::Continue(line, _) => *line,
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
        Stmt::IncDec { .. } | Stmt::Break(..) | Stmt::Continue(..) | Stmt::Fallthrough(_) => false,
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
