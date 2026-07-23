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
    /// When true, emit a per-statement `CallBuiltin(DBG_LINE)` marker so `--dap`
    /// can stop on statement lines. Normal runs leave this off (zero extra ops).
    debug: bool,
    /// True when the program contains an inline `rust {}` block (a
    /// `__rust_compile(...)` call), so a bare-name call may be an FFI export.
    has_ffi: bool,
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
        debug,
        has_ffi,
    };

    // ── main body (global scope) ──
    for s in &prog.main {
        c.stmt(s)?;
    }
    let end = c.b.current_pos();
    let exits = std::mem::take(&mut c.main_exits);
    for op in exits {
        c.b.patch_jump(op, end);
    }

    // ── subroutine bodies, emitted after main and jumped over ──
    if !prog.funcs.is_empty() {
        let skip = c.b.emit(Op::Jump(0), 0);
        for f in &prog.funcs {
            c.compile_func(f)?;
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

        // Prologue: pop args into their slots. The last argument is on top of
        // the stack, so bind slots high-to-low (receiver deepest, at slot 0).
        for i in (0..slot).rev() {
            self.b.emit(Op::SetSlot(i), f.line);
        }

        for s in &f.body {
            self.stmt(s)?;
        }
        // Fall-off: a function with no explicit `return` yields nil.
        self.b.emit(Op::LoadUndef, f.line);
        self.b.emit(Op::ReturnValue, f.line);

        self.scope = None;
        Ok(())
    }

    // ── variable access ────────────────────────────────────────────────────

    fn emit_get(&mut self, name: &str, line: u32) {
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

    fn emit_set(&mut self, name: &str, line: u32) {
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
                    Some(e) => self.emit_value(e)?,
                    None => self.emit_default(nt, *line),
                }
                self.types.insert(name.clone(), nt);
                self.decl_types.insert(name.clone(), decl_ty);
                self.emit_set(name, *line);
            }
            Stmt::Short {
                names,
                values,
                line,
            } => {
                if names.len() != values.len() {
                    return Err(format!(
                        "go-rs: assignment mismatch: {} variables but {} values (line {line})",
                        names.len(),
                        values.len()
                    ));
                }
                for (name, e) in names.iter().zip(values) {
                    let nt = self.infer(e);
                    let dt = self.type_name(e);
                    self.emit_value(e)?;
                    self.types.insert(name.clone(), nt);
                    self.decl_types.insert(name.clone(), dt);
                    self.emit_set(name, *line);
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
            Stmt::Return(val, line) => match self.scope {
                Some(_) => {
                    match val {
                        Some(e) => self.emit_value(e)?,
                        None => {
                            self.b.emit(Op::LoadUndef, *line);
                        }
                    }
                    self.b.emit(Op::ReturnValue, *line);
                }
                None => {
                    if let Some(e) = val {
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
                // `go f(args)` — only a direct call of a top-level function is
                // spawnable in this slice (no method/closure goroutines yet).
                match call {
                    Expr::Call { func, args, .. } => match func.as_ref() {
                        Expr::Ident(name) if self.funcs.contains_key(name) => {
                            for a in args {
                                self.emit_value(a)?;
                            }
                            let idx = self.b.add_name(name);
                            self.b.emit(Op::Go(idx, args.len() as u8), *line);
                        }
                        _ => {
                            return Err(format!(
                                "go-rs: `go` requires a top-level function call (line {line})"
                            ))
                        }
                    },
                    _ => {
                        return Err(format!(
                            "go-rs: `go` requires a function call (line {line})"
                        ))
                    }
                }
            }
            Stmt::Send { chan, val, line } => {
                self.expr(chan)?;
                self.expr(val)?;
                self.b.emit(Op::ChanSend, *line);
            }
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
                    self.emit_value(value)?;
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
        Ok(())
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
            | Expr::Recv { .. } => NumType::Unknown,
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
        | Stmt::Send { line, .. }
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
        Stmt::Return(v, _) => v.as_ref().is_some_and(expr_has_ffi),
        Stmt::If { then, els, .. } => body_has_ffi(then) || body_has_ffi(els),
        Stmt::For { body, .. } | Stmt::ForRange { body, .. } | Stmt::Block(body) => {
            body_has_ffi(body)
        }
        Stmt::Go { call, .. } => expr_has_ffi(call),
        Stmt::Send { chan, val, .. } => expr_has_ffi(chan) || expr_has_ffi(val),
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
