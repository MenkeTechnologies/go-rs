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
use std::collections::HashMap;

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
    /// Static types of the variables in the function currently being compiled.
    types: HashMap<String, NumType>,
    /// Every top-level function, by name (for call resolution).
    funcs: HashMap<String, FuncSig>,
    /// The stack of enclosing `for` loops (innermost last).
    loops: Vec<LoopScope>,
    /// `return`/jump-outs emitted inside `main`, patched to the end of `main`.
    main_exits: Vec<usize>,
}

/// Lower a whole program to a runnable chunk.
pub fn compile(prog: &Program) -> Result<Chunk, String> {
    let funcs = prog
        .funcs
        .iter()
        .map(|f| {
            (
                f.name.clone(),
                FuncSig {
                    arity: f.params.len(),
                    result: f
                        .results
                        .first()
                        .map(|t| numtype_of_ty(t))
                        .unwrap_or(NumType::Unknown),
                },
            )
        })
        .collect();

    let mut c = Compiler {
        b: ChunkBuilder::new(),
        scope: None,
        types: HashMap::new(),
        funcs,
        loops: Vec::new(),
        main_exits: Vec::new(),
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
        let name_idx = self.b.add_name(&f.name);
        self.b.add_sub_entry(name_idx, entry);

        let mut scope = Scope::new();
        self.types.clear();
        for (i, p) in f.params.iter().enumerate() {
            scope.slots.insert(p.name.clone(), i as u16);
            self.types.insert(p.name.clone(), numtype_of_ty(&p.ty));
        }
        scope.next_slot = f.params.len() as u16;
        self.scope = Some(scope);

        // Prologue: pop args into their slots. The last argument is on top of
        // the stack, so bind slots high-to-low.
        for i in (0..f.params.len()).rev() {
            self.b.emit(Op::SetSlot(i as u16), f.line);
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
                match init {
                    Some(e) => self.expr(e)?,
                    None => self.emit_default(nt, *line),
                }
                self.types.insert(name.clone(), nt);
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
                    self.expr(e)?;
                    self.types.insert(name.clone(), nt);
                    self.emit_set(name, *line);
                }
            }
            Stmt::Assign {
                target,
                op,
                value,
                line,
            } => {
                if *op == AssignOp::Set {
                    self.expr(value)?;
                } else {
                    self.emit_get(target, *line);
                    self.expr(value)?;
                    let l = self.types.get(target).copied().unwrap_or(NumType::Unknown);
                    let r = self.infer(value);
                    self.emit_arith(assign_binop(*op), l, r, *line);
                }
                self.emit_set(target, *line);
            }
            Stmt::IncDec { target, inc, line } => {
                self.emit_get(target, *line);
                self.b.emit(Op::LoadInt(1), *line);
                self.b.emit(if *inc { Op::Add } else { Op::Sub }, *line);
                self.emit_set(target, *line);
            }
            Stmt::ExprStmt(e) => {
                self.expr(e)?;
                // Every expression leaves exactly one value; a bare expression
                // statement discards it.
                self.b.emit(Op::Pop, 0);
            }
            Stmt::Return(val, line) => match &mut self.scope {
                Some(_) => {
                    match val {
                        Some(e) => self.expr(e)?,
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
            Expr::Selector { field, .. } => {
                return Err(format!("go-rs: unsupported selector `.{field}`"));
            }
        }
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
        // `pkg.Func(...)` — only `fmt` is wired in slice 1.
        if let Expr::Selector { recv, field } = func {
            if let Expr::Ident(pkg) = recv.as_ref() {
                let id = match (pkg.as_str(), field.as_str()) {
                    ("fmt", "Println") => host::GPRINTLN,
                    ("fmt", "Print") => host::GPRINT,
                    ("fmt", "Printf") => host::GPRINTF,
                    _ => {
                        return Err(format!(
                            "go-rs: unsupported call `{pkg}.{field}` (line {line})"
                        ))
                    }
                };
                for a in args {
                    self.expr(a)?;
                }
                self.b.emit(Op::CallBuiltin(id, args.len() as u8), line);
                return Ok(());
            }
        }

        // Bare-name call: a language builtin or a user function.
        if let Expr::Ident(name) = func {
            match name.as_str() {
                "println" => {
                    for a in args {
                        self.expr(a)?;
                    }
                    self.b
                        .emit(Op::CallBuiltin(host::GEPRINTLN, args.len() as u8), line);
                    return Ok(());
                }
                "print" => {
                    for a in args {
                        self.expr(a)?;
                    }
                    self.b
                        .emit(Op::CallBuiltin(host::GEPRINT, args.len() as u8), line);
                    return Ok(());
                }
                _ => {}
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
                    self.expr(a)?;
                }
                let idx = self.b.add_name(name);
                self.b.emit(Op::Call(idx, args.len() as u8), line);
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
            Expr::Call { func, .. } => match func.as_ref() {
                Expr::Ident(name) => self
                    .funcs
                    .get(name)
                    .map(|s| s.result)
                    .unwrap_or(NumType::Unknown),
                _ => NumType::Unknown,
            },
            Expr::Selector { .. } => NumType::Unknown,
        }
    }
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
