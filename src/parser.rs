//! A recursive-descent parser for the slice-1 Go grammar.
//!
//! Consumes the [`crate::lexer`] token stream — already carrying Go's
//! automatically-inserted semicolons — and builds a [`crate::ast::Program`].
//! The grammar covers a single-file `package main`: the package clause, imports,
//! `type T struct` declarations, and top-level `func` / method declarations whose
//! bodies use `:=`/`var`, assignment to lvalues (ident / index / field),
//! `if`/`else`, three-clause/condition/infinite `for`, `for … range`,
//! `return`/`break`/`continue`, and the usual expression forms including
//! composite literals (`[]T{…}`, `map[K]V{…}`, `T{…}`), indexing, `make`, and
//! `fmt.Println`-style selector calls.

use crate::ast::*;
use crate::lexer::{lex, Tok, Token};
use std::collections::HashSet;

/// The two kinds of `type` declaration the parser produces.
enum TypeDecl {
    Struct(StructDecl),
    Interface(InterfaceDecl),
}

/// Parse Go source into a [`Program`].
pub fn parse(src: &str) -> Result<Program, String> {
    let tokens = lex(src)?;
    // Pre-scan `type <IDENT> struct` so a `T{…}` composite literal can be told
    // apart from an identifier `T` followed by a block `{`.
    let mut struct_names = HashSet::new();
    for w in tokens.windows(3) {
        if matches!(w[0].kind, Tok::Type) && matches!(w[2].kind, Tok::Struct) {
            if let Tok::Ident(n) = &w[1].kind {
                struct_names.insert(n.clone());
            }
        }
    }
    let mut p = Parser {
        tokens,
        pos: 0,
        struct_names,
    };
    p.program()
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Names declared as `type T struct` — enables `T{…}` composite literals.
    struct_names: HashSet<String>,
}

impl Parser {
    // ── token cursor ───────────────────────────────────────────────────────

    fn peek(&self) -> &Tok {
        &self.tokens[self.pos].kind
    }

    fn peek2(&self) -> &Tok {
        self.tokens
            .get(self.pos + 1)
            .map(|t| &t.kind)
            .unwrap_or(&Tok::Eof)
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

        let mut types = Vec::new();
        let mut interfaces = Vec::new();
        let mut main = Vec::new();
        let mut funcs = Vec::new();
        while !matches!(self.peek(), Tok::Eof) {
            match self.peek() {
                Tok::Func => {
                    let f = self.func_decl()?;
                    if f.name == "main" && f.receiver.is_none() {
                        main = f.body;
                    } else {
                        funcs.push(f);
                    }
                }
                Tok::Type => match self.type_decl()? {
                    TypeDecl::Struct(s) => types.push(s),
                    TypeDecl::Interface(i) => interfaces.push(i),
                },
                // Package-level `var`/`const` declarations run in `main`'s global
                // scope (slice: no separate package-init phase).
                Tok::Var | Tok::Const => main.push(self.stmt()?),
                other => {
                    return Err(format!(
                        "go-rs: expected `func` or `type` at top level, found `{other}` on line {}",
                        self.line()
                    ))
                }
            }
            self.skip_semis();
        }

        Ok(Program {
            package,
            imports,
            types,
            interfaces,
            main,
            funcs,
        })
    }

    /// Parse `type T struct { … }` or `type I interface { … }`.
    fn type_decl(&mut self) -> Result<TypeDecl, String> {
        self.expect(&Tok::Type)?;
        let name = self.ident()?;
        match self.peek() {
            Tok::Interface => {
                self.advance();
                self.expect(&Tok::LBrace)?;
                self.skip_semis();
                let mut methods = Vec::new();
                while !matches!(self.peek(), Tok::RBrace | Tok::Eof) {
                    // `method(params) [results]` — record the name, skip the rest.
                    methods.push(self.ident()?);
                    self.expect(&Tok::LParen)?;
                    self.skip_balanced_parens()?;
                    while !matches!(self.peek(), Tok::Semi | Tok::RBrace) {
                        self.advance();
                    }
                    self.skip_semis();
                }
                self.expect(&Tok::RBrace)?;
                Ok(TypeDecl::Interface(InterfaceDecl { name, methods }))
            }
            _ => {
                self.expect(&Tok::Struct)?;
                self.expect(&Tok::LBrace)?;
                self.skip_semis();
                let mut fields = Vec::new();
                while !matches!(self.peek(), Tok::RBrace | Tok::Eof) {
                    // One or more field names sharing a type: `x, y int`.
                    let mut names = vec![self.ident()?];
                    while self.eat(&Tok::Comma) {
                        names.push(self.ident()?);
                    }
                    let ty = self.type_name()?;
                    for n in names {
                        fields.push(Param {
                            name: n,
                            ty: ty.clone(),
                        });
                    }
                    self.skip_semis();
                }
                self.expect(&Tok::RBrace)?;
                Ok(TypeDecl::Struct(StructDecl { name, fields }))
            }
        }
    }

    /// Consume tokens through the closing `)` of an already-opened paren group.
    fn skip_balanced_parens(&mut self) -> Result<(), String> {
        let mut depth = 1;
        while depth > 0 {
            match self.advance() {
                Tok::LParen => depth += 1,
                Tok::RParen => depth -= 1,
                Tok::Eof => return Err("go-rs: unterminated `(` in interface method".to_string()),
                _ => {}
            }
        }
        Ok(())
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
        // Optional method receiver: `func (r T) name(...)`.
        let receiver = if matches!(self.peek(), Tok::LParen) {
            self.advance();
            let rname = self.ident()?;
            let rty = self.type_name()?;
            self.expect(&Tok::RParen)?;
            Some(Param {
                name: rname,
                ty: rty,
            })
        } else {
            None
        };
        let name = self.ident()?;
        self.expect(&Tok::LParen)?;
        let params = self.params()?;
        self.expect(&Tok::RParen)?;
        let results = self.results()?;
        let body = self.block()?;
        Ok(Func {
            name,
            receiver,
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
            Some(Tok::Ident(_)) | Some(Tok::LBracket) | Some(Tok::Star) | Some(Tok::Chan)
        )
    }

    /// Parse a type name. Handles named types (`int`, `string`, `T`, …), slices
    /// (`[]T`), maps (`map[K]V`), and pointers (`*T`) as opaque strings for later
    /// typing.
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
            // `map[K]V` — `map` is a predeclared ident, not a keyword.
            Tok::Ident(n) if n == "map" => {
                self.advance();
                self.expect(&Tok::LBracket)?;
                let k = self.type_name()?;
                self.expect(&Tok::RBracket)?;
                let v = self.type_name()?;
                Ok(format!("map[{k}]{v}"))
            }
            // `chan T` — a channel type.
            Tok::Chan => {
                self.advance();
                Ok(format!("chan {}", self.type_name()?))
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
            Tok::Go => {
                let line = self.line();
                self.advance();
                let call = self.expr()?;
                Ok(Stmt::Go { call, line })
            }
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

        // `for [k[, v] :=|= ] range iter { … }` — detected by a `range` keyword
        // in the header (before the block `{`).
        if self.header_has_range() {
            return self.for_range(line);
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

    /// True if a `range` keyword appears in the current `for` header (before the
    /// block-opening `{`).
    fn header_has_range(&self) -> bool {
        let mut i = self.pos;
        while let Some(t) = self.tokens.get(i) {
            match t.kind {
                Tok::Range => return true,
                Tok::LBrace | Tok::Semi | Tok::Eof => return false,
                _ => i += 1,
            }
        }
        false
    }

    /// Parse a `for … range` loop header and body.
    fn for_range(&mut self, line: u32) -> Result<Stmt, String> {
        let mut key = None;
        let mut val = None;
        let mut define = false;
        if !matches!(self.peek(), Tok::Range) {
            // `k` or `k, v` then `:=` or `=`.
            let mut names = vec![self.range_name()?];
            if self.eat(&Tok::Comma) {
                names.push(self.range_name()?);
            }
            if self.eat(&Tok::Define) {
                define = true;
            } else if self.eat(&Tok::Assign) {
                define = false;
            } else {
                return Err(format!(
                    "go-rs: expected `:=` or `=` in range clause on line {}",
                    self.line()
                ));
            }
            key = names.first().cloned().flatten();
            val = names.get(1).cloned().flatten();
        }
        self.expect(&Tok::Range)?;
        let iter = self.expr()?;
        let body = self.block()?;
        Ok(Stmt::ForRange {
            key,
            val,
            define,
            iter,
            body,
            line,
        })
    }

    /// A range key/value name: an identifier, or `None` for the blank `_`.
    fn range_name(&mut self) -> Result<Option<String>, String> {
        let n = self.ident()?;
        Ok(if n == "_" { None } else { Some(n) })
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
            // `ch <- val` — channel send.
            Tok::Arrow => {
                self.advance();
                let val = self.expr()?;
                Ok(Stmt::Send {
                    chan: target,
                    val,
                    line,
                })
            }
            Tok::PlusPlus => {
                self.advance();
                Ok(Stmt::IncDec {
                    target,
                    inc: true,
                    line,
                })
            }
            Tok::MinusMinus => {
                self.advance();
                Ok(Stmt::IncDec {
                    target,
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
                    target,
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
            // `<-ch` — channel receive.
            Tok::Arrow => {
                self.advance();
                Ok(Expr::Recv {
                    chan: Box::new(self.unary()?),
                })
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
                Tok::LBracket => {
                    self.advance();
                    let index = self.expr()?;
                    self.expect(&Tok::RBracket)?;
                    e = Expr::Index {
                        recv: Box::new(e),
                        index: Box::new(index),
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
        // `[]T{ … }` slice composite literal.
        if matches!(self.peek(), Tok::LBracket) {
            return self.slice_literal();
        }
        match self.advance() {
            Tok::Int(n) => Ok(Expr::Int(n)),
            Tok::Float(f) => Ok(Expr::Float(f)),
            Tok::Str(s) => Ok(Expr::Str(s)),
            Tok::True => Ok(Expr::Bool(true)),
            Tok::False => Ok(Expr::Bool(false)),
            Tok::Ident(s) => {
                match s.as_str() {
                    // `map[K]V{ … }` map composite literal.
                    "map" if matches!(self.peek(), Tok::LBracket) => self.map_literal(),
                    // `make([]T, n)` / `make(map[K]V)`.
                    "make" if matches!(self.peek(), Tok::LParen) => self.make_expr(),
                    // `T{ … }` struct composite literal (T declared as a struct).
                    _ if matches!(self.peek(), Tok::LBrace) && self.struct_names.contains(&s) => {
                        self.struct_literal(s)
                    }
                    _ => Ok(Expr::Ident(s)),
                }
            }
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

    /// `[]T{ e0, e1, … }`.
    fn slice_literal(&mut self) -> Result<Expr, String> {
        self.expect(&Tok::LBracket)?;
        self.expect(&Tok::RBracket)?;
        let elem_ty = self.type_name()?;
        self.expect(&Tok::LBrace)?;
        let mut elems = Vec::new();
        while !matches!(self.peek(), Tok::RBrace) {
            elems.push(self.expr()?);
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(&Tok::RBrace)?;
        Ok(Expr::SliceLit { elem_ty, elems })
    }

    /// `map[K]V{ k0: v0, … }` (already consumed the `map` ident).
    fn map_literal(&mut self) -> Result<Expr, String> {
        self.expect(&Tok::LBracket)?;
        let key_ty = self.type_name()?;
        self.expect(&Tok::RBracket)?;
        let val_ty = self.type_name()?;
        self.expect(&Tok::LBrace)?;
        let mut pairs = Vec::new();
        while !matches!(self.peek(), Tok::RBrace) {
            let k = self.expr()?;
            self.expect(&Tok::Colon)?;
            let v = self.expr()?;
            pairs.push((k, v));
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(&Tok::RBrace)?;
        Ok(Expr::MapLit {
            key_ty,
            val_ty,
            pairs,
        })
    }

    /// `T{ … }` — positional `T{a, b}` or keyed `T{x: a, y: b}`.
    fn struct_literal(&mut self, type_name: String) -> Result<Expr, String> {
        self.expect(&Tok::LBrace)?;
        let mut fields = Vec::new();
        while !matches!(self.peek(), Tok::RBrace) {
            // Keyed element: `ident :` (but not `::`); otherwise positional.
            let key = if matches!(self.peek(), Tok::Ident(_)) && matches!(self.peek2(), Tok::Colon)
            {
                let n = self.ident()?;
                self.expect(&Tok::Colon)?;
                Some(n)
            } else {
                None
            };
            let val = self.expr()?;
            fields.push((key, val));
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(&Tok::RBrace)?;
        Ok(Expr::StructLit { type_name, fields })
    }

    /// `make([]T, len)` (slice) or `make(map[K]V)` (map). The first argument is a
    /// type; a slice may carry a length (and an ignored capacity).
    fn make_expr(&mut self) -> Result<Expr, String> {
        self.expect(&Tok::LParen)?;
        let ty = self.type_name()?;
        // `make(chan T, cap)` — a channel.
        if ty.starts_with("chan ") {
            let cap = if self.eat(&Tok::Comma) {
                Some(Box::new(self.expr()?))
            } else {
                None
            };
            self.expect(&Tok::RParen)?;
            return Ok(Expr::MakeChan { cap });
        }
        let is_map = ty.starts_with("map[");
        let mut len = None;
        if self.eat(&Tok::Comma) {
            len = Some(Box::new(self.expr()?));
            // An optional capacity argument is accepted and ignored.
            if self.eat(&Tok::Comma) {
                let _ = self.expr()?;
            }
        }
        self.expect(&Tok::RParen)?;
        let elem_ty = ty.strip_prefix("[]").unwrap_or("");
        Ok(Expr::Make {
            is_map,
            len,
            elem_zero: Box::new(zero_expr(elem_ty)),
        })
    }
}

/// The zero-value expression for a Go element type (drives `make([]T, n)` fill).
fn zero_expr(ty: &str) -> Expr {
    match ty {
        "float32" | "float64" => Expr::Float(0.0),
        "string" => Expr::Str(String::new()),
        "bool" => Expr::Bool(false),
        _ => Expr::Int(0),
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
