//! The Go abstract syntax tree go-rs parses into and lowers from.
//!
//! Models a single-file `package main` program: a package clause, imports,
//! `type T struct` declarations, and top-level `func` / method declarations.
//! `func main` is the entry point; every other `func` (and every method)
//! becomes a fusevm subroutine (see [`crate::compiler`]). The grammar covers
//! arithmetic/control flow, composite types (slices, maps, structs, interfaces,
//! indexing, `range`, composite literals), and concurrency (`go`, `chan`, `<-`,
//! `close`) lowered onto fusevm's cooperative scheduler.

/// A parsed Go source file.
#[derive(Debug, Clone)]
pub struct Program {
    /// The package name (`package main` → `"main"`).
    pub package: String,
    /// Imported package paths, in source order (e.g. `"fmt"`).
    pub imports: Vec<String>,
    /// `type T struct { … }` declarations.
    pub types: Vec<StructDecl>,
    /// `type I interface { … }` declarations (method-set names).
    pub interfaces: Vec<InterfaceDecl>,
    /// The body of `func main`, run in the global scope.
    pub main: Vec<Stmt>,
    /// Every top-level `func` other than `main` (including methods), lowered to
    /// subroutines.
    pub funcs: Vec<Func>,
}

/// A `type T struct { field T; … }` declaration.
#[derive(Debug, Clone)]
pub struct StructDecl {
    pub name: String,
    pub fields: Vec<Param>,
}

/// A `type I interface { m(...) ...; … }` declaration — its method-set names.
#[derive(Debug, Clone)]
pub struct InterfaceDecl {
    pub name: String,
    pub methods: Vec<String>,
}

/// A top-level function or method declaration.
#[derive(Debug, Clone)]
pub struct Func {
    pub name: String,
    /// The receiver for a method (`func (r T) m()`); `None` for a plain `func`.
    pub receiver: Option<Param>,
    pub params: Vec<Param>,
    /// Result types in declaration order.
    pub results: Vec<String>,
    /// Result names, aligned with `results` (`""` for an unnamed result). When
    /// any result is named, they become zero-initialized locals the body can read
    /// and assign, and a bare `return` yields their current values.
    pub result_names: Vec<String>,
    pub body: Vec<Stmt>,
    pub line: u32,
}

/// A single function parameter: a name and its declared Go type.
#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: String,
}

/// A statement.
#[derive(Debug, Clone)]
pub enum Stmt {
    /// `var name [T] [= init]`.
    Var {
        name: String,
        ty: Option<String>,
        init: Option<Expr>,
        line: u32,
    },
    /// `name, ... := expr, ...` (short variable declaration).
    Short {
        names: Vec<String>,
        values: Vec<Expr>,
        line: u32,
    },
    /// `target op= value` (op = `=`, `+=`, `-=`, `*=`, `/=`, `%=`). The target
    /// is an lvalue: an identifier, an index (`x[i]`), or a field (`x.f`).
    Assign {
        target: Expr,
        op: AssignOp,
        value: Expr,
        line: u32,
    },
    /// `t0, t1, … = v0, v1, …` — parallel assignment to existing lvalues. All
    /// right-hand sides are evaluated before any assignment (so `a, b = b, a`
    /// swaps).
    AssignMulti {
        targets: Vec<Expr>,
        values: Vec<Expr>,
        line: u32,
    },
    /// `target++` / `target--` (target is an lvalue expression).
    IncDec { target: Expr, inc: bool, line: u32 },
    /// A bare expression evaluated for effect (e.g. a call).
    ExprStmt(Expr),
    /// `return [expr [, expr …]]` — zero, one, or multiple result values.
    Return(Vec<Expr>, u32),
    /// `if [init;] cond { then } [else els]`.
    If {
        init: Option<Box<Stmt>>,
        cond: Expr,
        then: Vec<Stmt>,
        els: Vec<Stmt>,
        line: u32,
    },
    /// `for [init;] [cond;] [post] { body }` — covers the infinite, condition-
    /// only, and three-clause forms.
    For {
        init: Option<Box<Stmt>>,
        cond: Option<Expr>,
        post: Option<Box<Stmt>>,
        body: Vec<Stmt>,
        line: u32,
    },
    /// `for [key[, val]] := range iter { body }` over a slice, map, or string.
    /// `define` is true for `:=`, false for `=`; `key`/`val` are `None` for the
    /// blank identifier `_` or an omitted value.
    ForRange {
        key: Option<String>,
        val: Option<String>,
        define: bool,
        iter: Expr,
        body: Vec<Stmt>,
        line: u32,
    },
    /// `go f(args)` — spawn a goroutine running the named function.
    Go { call: Expr, line: u32 },
    /// `defer f(args)` — evaluate `f` and `args` now, call at function return in
    /// LIFO order.
    Defer { call: Expr, line: u32 },
    /// `ch <- val` — send `val` on channel `ch`.
    Send { chan: Expr, val: Expr, line: u32 },
    /// `select { case …: …; default: … }` over channel operations.
    Select {
        cases: Vec<SelectClause>,
        default: Option<Vec<Stmt>>,
        line: u32,
    },
    /// `switch [init;] [tag] { case …: …; default: … }`. With no `tag`, each
    /// case expression is a boolean condition (expression switch). Cases run the
    /// first match then break (no implicit fallthrough).
    Switch {
        init: Option<Box<Stmt>>,
        tag: Option<Expr>,
        cases: Vec<SwitchCase>,
        default: Option<Vec<Stmt>>,
        line: u32,
    },
    /// `break`.
    Break(u32),
    /// `continue`.
    Continue(u32),
    /// A `{ ... }` block.
    Block(Vec<Stmt>),
}

/// One `case` clause of a `switch`: the case expressions (`case a, b:` matches
/// either) and the statements to run.
#[derive(Debug, Clone)]
pub struct SwitchCase {
    pub exprs: Vec<Expr>,
    pub body: Vec<Stmt>,
}

/// One `case` clause of a `select`.
#[derive(Debug, Clone)]
pub struct SelectClause {
    pub comm: SelectComm,
    pub body: Vec<Stmt>,
}

/// The communication operation of a `select` case.
#[derive(Debug, Clone)]
pub enum SelectComm {
    /// `case [v :=] <-ch:` — receive, optionally binding the value.
    Recv { bind: Option<String>, chan: Expr },
    /// `case ch <- val:` — send.
    Send { chan: Expr, val: Expr },
}

/// Compound-assignment operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

/// An expression.
#[derive(Debug, Clone)]
pub enum Expr {
    Int(i64),
    /// A float literal: its `f64` value plus, when representable, an exact
    /// decimal `(mantissa, scale)` (`mantissa · 10⁻ˢᶜᵃˡᵉ`) for constant folding.
    Float(f64, Option<(i128, i32)>),
    Str(String),
    Bool(bool),
    Ident(String),
    /// A unary operation (`-x`, `!x`).
    Unary {
        op: UnOp,
        rhs: Box<Expr>,
    },
    /// A binary operation.
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// A call `func(args)`.
    Call {
        func: Box<Expr>,
        args: Vec<Expr>,
        line: u32,
    },
    /// A selector `recv.field` (e.g. `fmt.Println`, or a struct field read).
    Selector {
        recv: Box<Expr>,
        field: String,
    },
    /// An index `recv[index]` — slice element or map lookup.
    Index {
        recv: Box<Expr>,
        index: Box<Expr>,
    },
    /// A slice expression `recv[low:high]` (either bound optional: `s[lo:]`,
    /// `s[:hi]`, `s[:]`).
    Slice {
        recv: Box<Expr>,
        low: Option<Box<Expr>>,
        high: Option<Box<Expr>>,
    },
    /// A slice composite literal `[]T{elems}`.
    SliceLit {
        elem_ty: String,
        elems: Vec<Expr>,
    },
    /// A map composite literal `map[K]V{k: v, …}`.
    MapLit {
        key_ty: String,
        val_ty: String,
        pairs: Vec<(Expr, Expr)>,
    },
    /// A struct composite literal `T{…}` (positional or `field: value`).
    StructLit {
        type_name: String,
        fields: Vec<(Option<String>, Expr)>,
    },
    /// `make([]T, len)` (a slice) or `make(map[K]V)` (a map). `elem_zero` is the
    /// slice element's zero value.
    Make {
        is_map: bool,
        len: Option<Box<Expr>>,
        elem_zero: Box<Expr>,
    },
    /// `make(chan T, cap)` — a channel with buffer capacity `cap` (0 if omitted).
    MakeChan {
        cap: Option<Box<Expr>>,
    },
    /// `<-ch` — receive a value from channel `ch`.
    Recv {
        chan: Box<Expr>,
    },
    /// A function literal `func(params) results { body }` — a closure. Captured
    /// variables (free vars of the body) are bound by value at creation.
    FuncLit {
        params: Vec<Param>,
        results: Vec<String>,
        body: Vec<Stmt>,
    },
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
    /// `&x` — address-of. go-rs composite values are heap handles (reference
    /// types), so this is a no-copy reference.
    Addr,
    /// `*p` — pointer dereference; identity on a go-rs handle.
    Deref,
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
}
