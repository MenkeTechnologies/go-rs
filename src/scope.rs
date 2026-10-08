//! Block scoping for local variables.
//!
//! The compiler keys a function's variables by name — one slot, one recorded
//! type, one capture decision per name. Go scopes a variable to its block, so a
//! declaration in an inner block that reuses an outer variable's name is a
//! *different* variable:
//!
//! ```go
//! for j := 0; j < 3; j++ {
//!     for j := 10; j < 12; j++ { … }   // a new j; the outer loop still counts 0, 1, 2
//! }
//! ```
//!
//! Keyed by name, both `j`s were one slot and the inner loop clobbered the
//! outer one. This pass resolves scopes before the compiler sees the program:
//! every declaration that shadows a variable visible from an enclosing block of
//! the same function is renamed to a name nothing else uses (`j$1`), and every
//! reference inside its scope follows. A program without shadowing is returned
//! unchanged.
//!
//! What counts as an enclosing block is Go's: a function's parameters, receiver
//! and named results; each `{…}`; the implicit block of an `if` / `for` /
//! `switch` (holding its init statement) and the separate block of each body,
//! `else`, `case` and `select` clause; and a function literal, whose body sees
//! the variables of the function it is written in. `func main`'s body shares
//! the global scope in go-rs, so a name declared at its top level is not
//! renamed — only a re-declaration in a block nested inside it is.

use crate::ast::{Expr, Func, Program, SelectComm, Stmt};
use std::collections::HashMap;

/// Rename every shadowing local declaration in `prog` (see the module docs).
pub fn resolve(prog: &mut Program) {
    for f in &mut prog.funcs {
        resolve_func(f);
    }
    // `main`'s body: its top level is the global scope.
    let mut r = Resolver::default();
    r.push();
    r.top_is_global = true;
    r.stmts(&mut prog.main);
}

fn resolve_func(f: &mut Func) {
    let mut r = Resolver::default();
    r.push();
    let names = f
        .receiver
        .iter()
        .chain(f.params.iter())
        .map(|p| p.name.clone())
        .chain(f.result_names.iter().cloned());
    for n in names {
        r.bind(n);
    }
    r.stmts(&mut f.body);
}

#[derive(Default)]
struct Resolver {
    /// Innermost last: each block's source name → the name it compiles under.
    scopes: Vec<HashMap<String, String>>,
    /// Suffix counter for fresh names.
    next: usize,
    /// Whether `scopes[0]` is the global scope (`main`'s top level), whose
    /// declarations are never renamed.
    top_is_global: bool,
}

impl Resolver {
    fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop(&mut self) {
        self.scopes.pop();
    }

    /// Record `name` in the innermost scope under its own name.
    fn bind(&mut self, name: String) {
        if let Some(s) = self.scopes.last_mut() {
            s.insert(name.clone(), name);
        }
    }

    /// The name a reference to `name` compiles under.
    fn lookup(&self, name: &str) -> Option<&String> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }

    /// Declare `name` in the innermost scope and rewrite it to the name it
    /// compiles under. A name already in the innermost scope is the same
    /// variable (`a, err := …` after `x, err := …`); one visible from an
    /// enclosing scope is shadowed and gets a fresh name.
    fn declare(&mut self, name: &mut String) {
        if name == "_" {
            return;
        }
        let depth = self.scopes.len();
        if let Some(cur) = self.scopes.last().and_then(|s| s.get(name.as_str())) {
            *name = cur.clone();
            return;
        }
        let at_global_top = self.top_is_global && depth == 1;
        let shadows = !at_global_top && self.lookup(name).is_some();
        let source = name.clone();
        if shadows {
            self.next += 1;
            *name = format!("{source}${}", self.next);
        }
        if let Some(s) = self.scopes.last_mut() {
            s.insert(source, name.clone());
        }
    }

    /// Rewrite a reference to a variable.
    fn reference(&self, name: &mut String) {
        if let Some(n) = self.lookup(name) {
            if n != name {
                *name = n.clone();
            }
        }
    }

    /// A `{…}` block: its own scope.
    fn block(&mut self, body: &mut [Stmt]) {
        self.push();
        self.stmts(body);
        self.pop();
    }

    fn stmts(&mut self, body: &mut [Stmt]) {
        for s in body {
            self.stmt(s);
        }
    }

    fn opt_stmt(&mut self, s: &mut Option<Box<Stmt>>) {
        if let Some(s) = s {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &mut Stmt) {
        match s {
            // The initializer is resolved before the name is declared: in
            // `x := x + 1` the right-hand `x` is the outer one.
            Stmt::Var { name, init, .. } => {
                if let Some(e) = init {
                    self.expr(e);
                }
                self.declare(name);
            }
            Stmt::Short { names, values, .. } => {
                values.iter_mut().for_each(|e| self.expr(e));
                names.iter_mut().for_each(|n| self.declare(n));
            }
            Stmt::Assign { target, value, .. } => {
                self.expr(target);
                self.expr(value);
            }
            Stmt::AssignMulti {
                targets, values, ..
            } => {
                targets.iter_mut().for_each(|e| self.expr(e));
                values.iter_mut().for_each(|e| self.expr(e));
            }
            Stmt::IncDec { target, .. } => self.expr(target),
            Stmt::ExprStmt(e) => self.expr(e),
            Stmt::Return(es, _) => es.iter_mut().for_each(|e| self.expr(e)),
            Stmt::If {
                init,
                cond,
                then,
                els,
                ..
            } => {
                self.push();
                self.opt_stmt(init);
                self.expr(cond);
                self.block(then);
                self.block(els);
                self.pop();
            }
            Stmt::For {
                init,
                cond,
                post,
                body,
                ..
            } => {
                self.push();
                self.opt_stmt(init);
                if let Some(c) = cond {
                    self.expr(c);
                }
                self.opt_stmt(post);
                self.block(body);
                self.pop();
            }
            Stmt::ForRange {
                key,
                val,
                define,
                iter,
                body,
                ..
            } => {
                // The range expression is evaluated outside the loop's scope.
                self.expr(iter);
                self.push();
                for n in [key, val].into_iter().flatten() {
                    if *define {
                        self.declare(n);
                    } else {
                        self.reference(n);
                    }
                }
                self.block(body);
                self.pop();
            }
            Stmt::Go { call, .. } | Stmt::Defer { call, .. } => self.expr(call),
            Stmt::Send { chan, val, .. } => {
                self.expr(chan);
                self.expr(val);
            }
            Stmt::Select { cases, default, .. } => {
                for c in cases {
                    self.push();
                    match &mut c.comm {
                        SelectComm::Recv {
                            bind,
                            ok_bind,
                            chan,
                            define,
                        } => {
                            self.expr(chan);
                            for n in [bind, ok_bind].into_iter().flatten() {
                                if *define {
                                    self.declare(n);
                                } else {
                                    self.reference(n);
                                }
                            }
                        }
                        SelectComm::Send { chan, val } => {
                            self.expr(chan);
                            self.expr(val);
                        }
                    }
                    self.stmts(&mut c.body);
                    self.pop();
                }
                if let Some(d) = default {
                    self.block(d);
                }
            }
            Stmt::Switch {
                init,
                tag,
                cases,
                default,
                ..
            } => {
                self.push();
                self.opt_stmt(init);
                if let Some(t) = tag {
                    self.expr(t);
                }
                for c in cases {
                    c.exprs.iter_mut().for_each(|e| self.expr(e));
                    self.block(&mut c.body);
                }
                if let Some(d) = default {
                    self.block(d);
                }
                self.pop();
            }
            // `switch v := x.(type)`: `x` is resolved before `v` exists, and the
            // one `v` stands for the binding in every clause.
            Stmt::TypeSwitch {
                init,
                bind,
                expr,
                cases,
                default,
                ..
            } => {
                self.push();
                self.opt_stmt(init);
                self.expr(expr);
                self.push();
                if let Some(b) = bind {
                    self.declare(b);
                }
                for c in cases {
                    self.block(&mut c.body);
                }
                if let Some(d) = default {
                    self.block(d);
                }
                self.pop();
                self.pop();
            }
            Stmt::Block(body) => self.block(body),
            Stmt::Fallthrough(_)
            | Stmt::Goto(..)
            | Stmt::Label(..)
            | Stmt::Break(..)
            | Stmt::Continue(..) => {}
        }
    }

    fn opt_expr(&mut self, e: &mut Option<Box<Expr>>) {
        if let Some(e) = e {
            self.expr(e);
        }
    }

    fn expr(&mut self, e: &mut Expr) {
        match e {
            Expr::Ident(n) => self.reference(n),
            Expr::Int(_) | Expr::Float(..) | Expr::Str(_) | Expr::Bool(_) => {}
            Expr::Unary { rhs, .. } => self.expr(rhs),
            Expr::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            Expr::Call { func, args, .. } => {
                self.expr(func);
                args.iter_mut().for_each(|a| self.expr(a));
            }
            Expr::Selector { recv, .. } => self.expr(recv),
            Expr::Index { recv, index } => {
                self.expr(recv);
                self.expr(index);
            }
            Expr::Slice {
                recv,
                low,
                high,
                max,
            } => {
                self.expr(recv);
                self.opt_expr(low);
                self.opt_expr(high);
                self.opt_expr(max);
            }
            Expr::SliceLit { elems, .. } => elems.iter_mut().for_each(|e| self.expr(e)),
            Expr::MapLit { pairs, .. } => {
                for (k, v) in pairs {
                    self.expr(k);
                    self.expr(v);
                }
            }
            Expr::StructLit { fields, .. } => fields.iter_mut().for_each(|(_, v)| self.expr(v)),
            Expr::Make {
                len,
                cap,
                elem_zero,
                ..
            } => {
                self.opt_expr(len);
                self.opt_expr(cap);
                self.expr(elem_zero);
            }
            Expr::MakeChan { cap, .. } => self.opt_expr(cap),
            Expr::Recv { chan } => self.expr(chan),
            // A function literal sees the enclosing function's variables; its
            // parameters are its outermost scope.
            Expr::FuncLit { params, body, .. } => {
                self.push();
                for p in params.iter_mut() {
                    self.declare(&mut p.name);
                }
                self.stmts(body);
                self.pop();
            }
            Expr::TypeAssert { expr, .. } => self.expr(expr),
        }
    }
}
