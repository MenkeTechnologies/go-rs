//! The Go abstract syntax tree go-rs parses into and lowers from.
//!
//! Slice 1 models a single-file `package main` program: a package clause, a set
//! of imports, and top-level `func` declarations. `func main` is the entry
//! point; every other `func` becomes a fusevm subroutine (see
//! [`crate::compiler`]). There are no structs, methods, interfaces, channels, or
//! goroutines yet — those grow in later waves.

/// A parsed Go source file.
#[derive(Debug, Clone)]
pub struct Program {
    /// The package name (`package main` → `"main"`).
    pub package: String,
    /// Imported package paths, in source order (e.g. `"fmt"`).
    pub imports: Vec<String>,
    /// The body of `func main`, run in the global scope.
    pub main: Vec<Stmt>,
    /// Every top-level `func` other than `main`, lowered to subroutines.
    pub funcs: Vec<Func>,
}

/// A top-level function declaration.
#[derive(Debug, Clone)]
pub struct Func {
    pub name: String,
    pub params: Vec<Param>,
    /// Result types in declaration order (names, if any, are dropped in slice 1).
    pub results: Vec<String>,
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
    /// `target op= value` (op = `=`, `+=`, `-=`, `*=`, `/=`, `%=`).
    Assign {
        target: String,
        op: AssignOp,
        value: Expr,
        line: u32,
    },
    /// `target++` / `target--`.
    IncDec {
        target: String,
        inc: bool,
        line: u32,
    },
    /// A bare expression evaluated for effect (e.g. a call).
    ExprStmt(Expr),
    /// `return [expr]` — slice 1 supports a single result value.
    Return(Option<Expr>, u32),
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
    /// `break`.
    Break(u32),
    /// `continue`.
    Continue(u32),
    /// A `{ ... }` block.
    Block(Vec<Stmt>),
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
    Float(f64),
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
    /// A selector `recv.field` (e.g. `fmt.Println`).
    Selector {
        recv: Box<Expr>,
        field: String,
    },
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
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
