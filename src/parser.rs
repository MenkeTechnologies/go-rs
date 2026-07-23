//! A recursive-descent parser for the slice-1 Go grammar.
//!
//! Consumes the [`crate::lexer`] token stream — already carrying Go's
//! automatically-inserted semicolons — and builds a [`crate::ast::Program`].
//! The grammar covers a single-file `package main`: the package clause, imports,
//! and top-level `func` declarations whose bodies use `:=`/`var`, assignment,
//! `if`/`else`, three-clause/condition/infinite `for`, `return`/`break`/
//! `continue`, and the usual expression forms including `fmt.Println`-style
//! selector calls.

use crate::ast::*;
use crate::lexer::{lex, Tok, Token};

/// Parse Go source into a [`Program`].
pub fn parse(src: &str) -> Result<Program, String> {
    let tokens = lex(src)?;
    let mut p = Parser { tokens, pos: 0 };
    p.program()
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    // ── token cursor ───────────────────────────────────────────────────────

    fn peek(&self) -> &Tok {
        &self.tokens[self.pos].kind
    }

    fn line(&self) -> u32 {
        self.tokens[self.pos].line
    }

    fn advance(&mut self) -> Tok {
        let t = self.tokens[self.pos].kind.clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        t
    }

    /// Consume the token if it matches `kind`; return whether it did.
    fn eat(&mut self, kind: &Tok) -> bool {
        if self.peek() == kind {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: &Tok) -> Result<(), String> {
        if self.peek() == kind {
            self.advance();
            Ok(())
        } else {
            Err(format!(
                "go-rs: expected `{kind}`, found `{}` on line {}",
                self.peek(),
                self.line()
            ))
        }
    }

    fn ident(&mut self) -> Result<String, String> {
        match self.advance() {
            Tok::Ident(s) => Ok(s),
            other => Err(format!(
                "go-rs: expected identifier, found `{other}` on line {}",
                self.line()
            )),
        }
    }

    /// Skip any run of statement-terminating semicolons (ASI-inserted or explicit).
    fn skip_semis(&mut self) {
        while matches!(self.peek(), Tok::Semi) {
            self.advance();
        }
    }

    // ── top level ──────────────────────────────────────────────────────────

    fn program(&mut self) -> Result<Program, String> {
        self.skip_semis();
        self.expect(&Tok::Package)?;
        let package = self.ident()?;
        self.skip_semis();

        let mut imports = Vec::new();
        while matches!(self.peek(), Tok::Import) {
            self.advance();
            self.parse_import(&mut imports)?;
            self.skip_semis();
        }

        let mut main = Vec::new();
        let mut funcs = Vec::new();
        while !matches!(self.peek(), Tok::Eof) {
            match self.peek() {
                Tok::Func => {
                    let f = self.func_decl()?;
                    if f.name == "main" {
                        main = f.body;
                    } else {
                        funcs.push(f);
                    }
                }
                other => {
                    return Err(format!(
                        "go-rs: expected `func` at top level, found `{other}` on line {}",
                        self.line()
                    ))
                }
            }
            self.skip_semis();
        }

        Ok(Program {
            package,
            imports,
            main,
            funcs,
        })
    }

    fn parse_import(&mut self, imports: &mut Vec<String>) -> Result<(), String> {
        // `import "path"` or `import ( "a"; "b" )`.
        if self.eat(&Tok::LParen) {
            self.skip_semis();
            while !matches!(self.peek(), Tok::RParen) {
                imports.push(self.import_path()?);
                self.skip_semis();
            }
            self.expect(&Tok::RParen)?;
        } else {
            imports.push(self.import_path()?);
        }
        Ok(())
    }

    fn import_path(&mut self) -> Result<String, String> {
        // Slice 1 ignores import aliases; a leading alias ident is consumed.
        if matches!(self.peek(), Tok::Ident(_)) {
            self.advance();
        }
        match self.advance() {
            Tok::Str(s) => Ok(s),
            other => Err(format!(
                "go-rs: expected import path string, found `{other}` on line {}",
                self.line()
            )),
        }
    }

    fn func_decl(&mut self) -> Result<Func, String> {
        let line = self.line();
        self.expect(&Tok::Func)?;
        let name = self.ident()?;
        self.expect(&Tok::LParen)?;
        let params = self.params()?;
        self.expect(&Tok::RParen)?;
        let results = self.results()?;
        let body = self.block()?;
        Ok(Func {
            name,
            params,
            results,
            body,
            line,
        })
    }

    /// Parse a parameter list. Supports grouped parameters that share a type
    /// (`a, b int`): names without a trailing type inherit the next typed
    /// group's type, matching Go's rule.
    fn params(&mut self) -> Result<Vec<Param>, String> {
        let mut params = Vec::new();
        if matches!(self.peek(), Tok::RParen) {
            return Ok(params);
        }
        // Collect (name, optional-type) entries, then back-fill inherited types.
        let mut pending: Vec<(String, Option<String>)> = Vec::new();
        loop {
            let name = self.ident()?;
            let ty = if self.type_starts() {
                Some(self.type_name()?)
            } else {
                None
            };
            let had_type = ty.is_some();
            pending.push((name, ty));
            if had_type {
                // Flush this group: everyone pending shares the just-read type.
                let t = pending.last().unwrap().1.clone().unwrap();
                for (n, slot) in pending.drain(..) {
                    params.push(Param {
                        name: n,
                        ty: slot.unwrap_or_else(|| t.clone()),
                    });
                }
            }
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        // Any trailing un-typed names are an error in real Go; slice 1 defaults
        // them to a dynamic type rather than rejecting.
        for (n, slot) in pending {
            params.push(Param {
                name: n,
                ty: slot.unwrap_or_else(|| "any".to_string()),
            });
        }
        Ok(params)
    }

    /// Parse a function result signature: nothing, a single type, or a
    /// parenthesized list of types.
    fn results(&mut self) -> Result<Vec<String>, String> {
        if matches!(self.peek(), Tok::LBrace) {
            return Ok(Vec::new());
        }
        if self.eat(&Tok::LParen) {
            let mut out = Vec::new();
            while !matches!(self.peek(), Tok::RParen) {
                // A named result `n int` — drop the name, keep the type.
                if matches!(self.peek(), Tok::Ident(_)) && self.type_starts_at(self.pos + 1) {
                    self.advance();
                }
                out.push(self.type_name()?);
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
            self.expect(&Tok::RParen)?;
            Ok(out)
        } else {
            Ok(vec![self.type_name()?])
        }
    }

    /// True if the current token can begin a type in a parameter/result position.
    fn type_starts(&self) -> bool {
        self.type_starts_at(self.pos)
    }

    fn type_starts_at(&self, pos: usize) -> bool {
        matches!(
            self.tokens.get(pos).map(|t| &t.kind),
            Some(Tok::Ident(_)) | Some(Tok::LBracket) | Some(Tok::Star)
        )
    }

    /// Parse a type name. Slice 1 handles named types (`int`, `string`, …),
    /// slices (`[]T`), and pointers (`*T`) as opaque strings for later typing.
    fn type_name(&mut self) -> Result<String, String> {
        match self.peek() {
            Tok::LBracket => {
                self.advance();
                self.expect(&Tok::RBracket)?;
                Ok(format!("[]{}", self.type_name()?))
            }
            Tok::Star => {
                self.advance();
                Ok(format!("*{}", self.type_name()?))
            }
            Tok::Ident(_) => self.ident(),
            other => Err(format!(
                "go-rs: expected type, found `{other}` on line {}",
                self.line()
            )),
        }
    }

    // ── statements ─────────────────────────────────────────────────────────

    fn block(&mut self) -> Result<Vec<Stmt>, String> {
        self.expect(&Tok::LBrace)?;
        self.skip_semis();
        let mut stmts = Vec::new();
        while !matches!(self.peek(), Tok::RBrace | Tok::Eof) {
            stmts.push(self.stmt()?);
            self.skip_semis();
        }
        self.expect(&Tok::RBrace)?;
        Ok(stmts)
    }

    fn stmt(&mut self) -> Result<Stmt, String> {
        match self.peek() {
            Tok::Var => self.var_stmt(),
            Tok::Return => {
                let line = self.line();
                self.advance();
                let val = if matches!(self.peek(), Tok::Semi | Tok::RBrace) {
                    None
                } else {
                    Some(self.expr()?)
                };
                Ok(Stmt::Return(val, line))
            }
            Tok::Break => {
                let line = self.line();
                self.advance();
                Ok(Stmt::Break(line))
            }
            Tok::Continue => {
                let line = self.line();
                self.advance();
                Ok(Stmt::Continue(line))
            }
            Tok::If => self.if_stmt(),
            Tok::For => self.for_stmt(),
            Tok::LBrace => Ok(Stmt::Block(self.block()?)),
            _ => self.simple_stmt(),
        }
    }

    fn var_stmt(&mut self) -> Result<Stmt, String> {
        let line = self.line();
        self.expect(&Tok::Var)?;
        let name = self.ident()?;
        let ty = if self.type_starts() && !matches!(self.peek(), Tok::Assign) {
            Some(self.type_name()?)
        } else {
            None
        };
        let init = if self.eat(&Tok::Assign) {
            Some(self.expr()?)
        } else {
            None
        };
        Ok(Stmt::Var {
            name,
            ty,
            init,
            line,
        })
    }

    fn if_stmt(&mut self) -> Result<Stmt, String> {
        let line = self.line();
        self.expect(&Tok::If)?;
        // Optional init clause: `if x := f(); cond { … }`.
        let mut init = None;
        let first = self.simple_stmt()?;
        let cond = if self.eat(&Tok::Semi) {
            init = Some(Box::new(first));
            self.expr()?
        } else {
            stmt_into_expr(first, line)?
        };
        let then = self.block()?;
        let els = if self.eat(&Tok::Else) {
            if matches!(self.peek(), Tok::If) {
                vec![self.if_stmt()?]
            } else {
                self.block()?
            }
        } else {
            Vec::new()
        };
        Ok(Stmt::If {
            init,
            cond,
            then,
            els,
            line,
        })
    }

    fn for_stmt(&mut self) -> Result<Stmt, String> {
        let line = self.line();
        self.expect(&Tok::For)?;

        // `for { … }` — infinite loop.
        if matches!(self.peek(), Tok::LBrace) {
            let body = self.block()?;
            return Ok(Stmt::For {
                init: None,
                cond: None,
                post: None,
                body,
                line,
            });
        }

        // Otherwise a condition-only or three-clause header.
        let first = if matches!(self.peek(), Tok::Semi) {
            None
        } else {
            Some(self.simple_stmt()?)
        };

        if self.eat(&Tok::Semi) {
            // Three-clause: `for init; cond; post { … }`.
            let cond = if matches!(self.peek(), Tok::Semi) {
                None
            } else {
                Some(self.expr()?)
            };
            self.expect(&Tok::Semi)?;
            let post = if matches!(self.peek(), Tok::LBrace) {
                None
            } else {
                Some(Box::new(self.simple_stmt()?))
            };
            let body = self.block()?;
            Ok(Stmt::For {
                init: first.map(Box::new),
                cond,
                post,
                body,
                line,
            })
        } else {
            // Condition-only: `for cond { … }`.
            let cond = match first {
                Some(s) => Some(stmt_into_expr(s, line)?),
                None => None,
            };
            let body = self.block()?;
            Ok(Stmt::For {
                init: None,
                cond,
                post: None,
                body,
                line,
            })
        }
    }

    /// Parse a simple statement: short decl, assignment, inc/dec, or a bare
    /// expression.
    fn simple_stmt(&mut self) -> Result<Stmt, String> {
        let line = self.line();

        // Short variable declaration: `a, b := …`. Look for an ident list
        // followed by `:=`.
        if let Some(names) = self.try_ident_list_define() {
            let mut values = vec![self.expr()?];
            while self.eat(&Tok::Comma) {
                values.push(self.expr()?);
            }
            return Ok(Stmt::Short {
                names,
                values,
                line,
            });
        }

        let target = self.expr()?;
        match self.peek() {
            Tok::PlusPlus => {
                self.advance();
                Ok(Stmt::IncDec {
                    target: expr_into_ident(target, line)?,
                    inc: true,
                    line,
                })
            }
            Tok::MinusMinus => {
                self.advance();
                Ok(Stmt::IncDec {
                    target: expr_into_ident(target, line)?,
                    inc: false,
                    line,
                })
            }
            Tok::Assign
            | Tok::PlusAssign
            | Tok::MinusAssign
            | Tok::StarAssign
            | Tok::SlashAssign
            | Tok::PercentAssign => {
                let op = match self.advance() {
                    Tok::Assign => AssignOp::Set,
                    Tok::PlusAssign => AssignOp::Add,
                    Tok::MinusAssign => AssignOp::Sub,
                    Tok::StarAssign => AssignOp::Mul,
                    Tok::SlashAssign => AssignOp::Div,
                    Tok::PercentAssign => AssignOp::Mod,
                    _ => unreachable!(),
                };
                let value = self.expr()?;
                Ok(Stmt::Assign {
                    target: expr_into_ident(target, line)?,
                    op,
                    value,
                    line,
                })
            }
            _ => Ok(Stmt::ExprStmt(target)),
        }
    }

    /// If the tokens at the cursor form `ident {, ident} :=`, consume them and
    /// return the names; otherwise leave the cursor unmoved and return `None`.
    fn try_ident_list_define(&mut self) -> Option<Vec<String>> {
        let start = self.pos;
        let mut names = Vec::new();
        loop {
            match self.peek() {
                Tok::Ident(s) => {
                    names.push(s.clone());
                    self.advance();
                }
                _ => {
                    self.pos = start;
                    return None;
                }
            }
            match self.peek() {
                Tok::Comma => {
                    self.advance();
                }
                Tok::Define => {
                    self.advance();
                    return Some(names);
                }
                _ => {
                    self.pos = start;
                    return None;
                }
            }
        }
    }

    // ── expressions (precedence climbing) ──────────────────────────────────

    fn expr(&mut self) -> Result<Expr, String> {
        self.binary(0)
    }

    fn binary(&mut self, min_prec: u8) -> Result<Expr, String> {
        let mut lhs = self.unary()?;
        while let Some((op, prec)) = binop_of(self.peek()) {
            if prec < min_prec {
                break;
            }
            self.advance();
            let rhs = self.binary(prec + 1)?;
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Tok::Minus => {
                self.advance();
                Ok(Expr::Unary {
                    op: UnOp::Neg,
                    rhs: Box::new(self.unary()?),
                })
            }
            Tok::Not => {
                self.advance();
                Ok(Expr::Unary {
                    op: UnOp::Not,
                    rhs: Box::new(self.unary()?),
                })
            }
            // Unary `+` is a no-op on numbers.
            Tok::Plus => {
                self.advance();
                self.unary()
            }
            _ => self.postfix(),
        }
    }

    fn postfix(&mut self) -> Result<Expr, String> {
        let mut e = self.primary()?;
        loop {
            match self.peek() {
                Tok::Dot => {
                    self.advance();
                    let field = self.ident()?;
                    e = Expr::Selector {
                        recv: Box::new(e),
                        field,
                    };
                }
                Tok::LParen => {
                    let line = self.line();
                    self.advance();
                    let mut args = Vec::new();
                    while !matches!(self.peek(), Tok::RParen) {
                        args.push(self.expr()?);
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    self.expect(&Tok::RParen)?;
                    e = Expr::Call {
                        func: Box::new(e),
                        args,
                        line,
                    };
                }
                _ => break,
            }
        }
        Ok(e)
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let line = self.line();
        match self.advance() {
            Tok::Int(n) => Ok(Expr::Int(n)),
            Tok::Float(f) => Ok(Expr::Float(f)),
            Tok::Str(s) => Ok(Expr::Str(s)),
            Tok::True => Ok(Expr::Bool(true)),
            Tok::False => Ok(Expr::Bool(false)),
            Tok::Ident(s) => Ok(Expr::Ident(s)),
            Tok::LParen => {
                let e = self.expr()?;
                self.expect(&Tok::RParen)?;
                Ok(e)
            }
            other => Err(format!(
                "go-rs: unexpected token `{other}` in expression on line {line}"
            )),
        }
    }
}

/// The binary operator and its precedence for a token, or `None` if the token
/// is not a binary operator. Higher binds tighter (Go's five levels).
fn binop_of(t: &Tok) -> Option<(BinOp, u8)> {
    Some(match t {
        Tok::OrOr => (BinOp::Or, 1),
        Tok::AndAnd => (BinOp::And, 2),
        Tok::EqEq => (BinOp::Eq, 3),
        Tok::NotEq => (BinOp::Ne, 3),
        Tok::Lt => (BinOp::Lt, 3),
        Tok::Gt => (BinOp::Gt, 3),
        Tok::Le => (BinOp::Le, 3),
        Tok::Ge => (BinOp::Ge, 3),
        Tok::Plus => (BinOp::Add, 4),
        Tok::Minus => (BinOp::Sub, 4),
        Tok::Star => (BinOp::Mul, 5),
        Tok::Slash => (BinOp::Div, 5),
        Tok::Percent => (BinOp::Mod, 5),
        _ => return None,
    })
}

/// Extract the expression from a statement that is required to be an expression
/// (a `for`/`if` condition parsed as a simple statement).
fn stmt_into_expr(s: Stmt, line: u32) -> Result<Expr, String> {
    match s {
        Stmt::ExprStmt(e) => Ok(e),
        _ => Err(format!(
            "go-rs: expected a boolean expression on line {line}"
        )),
    }
}

/// Require an assignment/inc-dec target to be a bare identifier (slice 1 has no
/// index/field lvalues).
fn expr_into_ident(e: Expr, line: u32) -> Result<String, String> {
    match e {
        Expr::Ident(s) => Ok(s),
        _ => Err(format!(
            "go-rs: cannot assign to non-identifier on line {line}"
        )),
    }
}
