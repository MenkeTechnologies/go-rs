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
use std::collections::{HashMap, HashSet};

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
    let struct_names = scan_struct_names(&tokens);
    // Pre-scan generic declarations — a `func Name[…]` / `type Name[…]` (Go 1.18
    // type parameters). go-rs is dynamically typed on the fusevm value model, so
    // generics are handled by *erasure*: the type-parameter and type-argument
    // brackets are consumed and dropped. Recording the generic names lets a use
    // site (`Name[int](…)`) tell an instantiation apart from an index `xs[i]`.
    let generic_names = scan_generic_names(&tokens);
    let mut p = Parser {
        tokens,
        pos: 0,
        struct_names,
        generic_names,
        no_composite: false,
        anon_structs: HashMap::new(),
    };
    p.program()
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Names declared as `type T struct` — enables `T{…}` composite literals.
    struct_names: HashSet<String>,
    /// Names declared generic (`func F[…]` / `type T[…]`) — enables telling an
    /// instantiation `F[int](…)` apart from an index expression `xs[i]`.
    generic_names: HashSet<String>,
    /// True while parsing a control-clause header (`if`/`for`/`switch` init and
    /// condition), where a `{` starts the body — so a bare `T{…}` / `pkg.T{…}`
    /// composite literal is suppressed (Go's `exprLev` rule; use parens to force
    /// one). Reset to `false` inside parentheses and the composite's own braces.
    no_composite: bool,
    /// Anonymous struct types encountered (`struct{…}` in a type or literal
    /// position), keyed by a canonical name synthesized from their fields, so an
    /// identical shape shares one type. Merged into the program's declared types.
    anon_structs: HashMap<String, Vec<Param>>,
}

/// Collect names declared as `type Name struct` — including generic structs
/// `type Name[T any] struct`, where an optional `[ … ]` type-parameter list sits
/// between the name and `struct`. Enables `Name{…}` composite-literal parsing.
fn scan_struct_names(tokens: &[Token]) -> HashSet<String> {
    let mut names = HashSet::new();
    let mut i = 0;
    while i + 2 < tokens.len() {
        if matches!(tokens[i].kind, Tok::Type) {
            if let Tok::Ident(n) = &tokens[i + 1].kind {
                // Step past an optional `[ … ]` generic type-parameter list.
                let mut j = i + 2;
                if matches!(tokens.get(j).map(|t| &t.kind), Some(Tok::LBracket)) {
                    let mut depth = 0;
                    while j < tokens.len() {
                        match &tokens[j].kind {
                            Tok::LBracket => depth += 1,
                            Tok::RBracket => {
                                depth -= 1;
                                if depth == 0 {
                                    j += 1;
                                    break;
                                }
                            }
                            _ => {}
                        }
                        j += 1;
                    }
                }
                if matches!(tokens.get(j).map(|t| &t.kind), Some(Tok::Struct)) {
                    names.insert(n.clone());
                }
            }
        }
        i += 1;
    }
    names
}

/// Collect the names of generic declarations: `func Name[…]` (with or without a
/// method receiver) and `type Name[…]`. Used to erase type arguments at use
/// sites without mistaking them for indexing.
fn scan_generic_names(tokens: &[Token]) -> HashSet<String> {
    let mut names = HashSet::new();
    let kind = |i: usize| tokens.get(i).map(|t| &t.kind);
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i].kind {
            Tok::Type => {
                // `type Name [` …
                if let (Some(Tok::Ident(n)), Some(Tok::LBracket)) = (kind(i + 1), kind(i + 2)) {
                    names.insert(n.clone());
                }
            }
            Tok::Func => {
                // Skip an optional method receiver `( … )` to reach the name.
                let mut j = i + 1;
                if matches!(kind(j), Some(Tok::LParen)) {
                    let mut depth = 0;
                    while j < tokens.len() {
                        match &tokens[j].kind {
                            Tok::LParen => depth += 1,
                            Tok::RParen => {
                                depth -= 1;
                                if depth == 0 {
                                    j += 1;
                                    break;
                                }
                            }
                            _ => {}
                        }
                        j += 1;
                    }
                }
                if let (Some(Tok::Ident(n)), Some(Tok::LBracket)) = (kind(j), kind(j + 1)) {
                    names.insert(n.clone());
                }
            }
            _ => {}
        }
        i += 1;
    }
    names
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
        // Package-level `var`/`const` declarations, collected separately so
        // `func main` (which sets the main body) does not overwrite them; they
        // are prepended to the main body to run first (no separate init phase).
        let mut globals = Vec::new();
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
                Tok::Type => {
                    if let Some(td) = self.type_decl()? {
                        match td {
                            TypeDecl::Struct(s) => types.push(s),
                            TypeDecl::Interface(i) => interfaces.push(i),
                        }
                    }
                }
                Tok::Var | Tok::Const => globals.push(self.stmt()?),
                other => {
                    return Err(format!(
                        "go-rs: expected `func` or `type` at top level, found `{other}` on line {}",
                        self.line()
                    ))
                }
            }
            self.skip_semis();
        }

        // Globals run before the main body.
        globals.extend(main);
        let main = globals;

        // Register anonymous struct types (`struct{…}` used as a type or literal)
        // so the compiler can zero-fill and field-access them.
        for (name, fields) in std::mem::take(&mut self.anon_structs) {
            types.push(StructDecl { name, fields });
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

    /// Parse `type T struct { … }`, `type I interface { … }`, or a defined type
    /// `type T <base>` (an alias in go-rs's dynamic model — parsed and discarded,
    /// so `Weekday` in `type Weekday int` is transparent). Returns `None` for a
    /// discarded defined type.
    fn type_decl(&mut self) -> Result<Option<TypeDecl>, String> {
        self.expect(&Tok::Type)?;
        let name = self.ident()?;
        // Erase a generic type-parameter list: `type Stack[T any] struct{…}`.
        if matches!(self.peek(), Tok::LBracket) {
            self.skip_type_brackets()?;
        }
        // A defined type over a non-struct base: `type Weekday int`,
        // `type Celsius float64`, `type ID string`. Consume the base and discard.
        if !matches!(self.peek(), Tok::Struct | Tok::Interface) {
            let _ = self.type_name()?;
            return Ok(None);
        }
        match self.peek() {
            Tok::Interface => {
                self.advance();
                self.expect(&Tok::LBrace)?;
                self.skip_semis();
                let mut methods = Vec::new();
                while !matches!(self.peek(), Tok::RBrace | Tok::Eof) {
                    // An element is either a method (`name(params) results`) or a
                    // generic type-constraint term (`~int | ~float64`, an embedded
                    // constraint name). A method is exactly `Ident (`; anything
                    // else is a constraint term, which go-rs erases.
                    if matches!(self.peek(), Tok::Ident(_)) && matches!(self.peek2(), Tok::LParen) {
                        methods.push(self.ident()?);
                        self.expect(&Tok::LParen)?;
                        self.skip_balanced_parens()?;
                    }
                    // Skip to the end of the element (method results, or the whole
                    // constraint term including `~`, `|`, and bracketed types).
                    while !matches!(self.peek(), Tok::Semi | Tok::RBrace | Tok::Eof) {
                        self.advance();
                    }
                    self.skip_semis();
                }
                self.expect(&Tok::RBrace)?;
                Ok(Some(TypeDecl::Interface(InterfaceDecl { name, methods })))
            }
            _ => {
                self.expect(&Tok::Struct)?;
                let fields = self.struct_field_list()?;
                Ok(Some(TypeDecl::Struct(StructDecl { name, fields })))
            }
        }
    }

    /// Parse a `{ field-decls }` struct body into its fields. The `struct`
    /// keyword is already consumed; the opening `{` is the current token.
    fn struct_field_list(&mut self) -> Result<Vec<Param>, String> {
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
        Ok(fields)
    }

    /// Parse an anonymous `struct{ … }` type (the `struct` keyword is the current
    /// token), register it under a canonical name synthesized from its fields,
    /// and return that name. Identical shapes share one type.
    fn anon_struct_type(&mut self) -> Result<String, String> {
        self.expect(&Tok::Struct)?;
        let fields = self.struct_field_list()?;
        let inner = fields
            .iter()
            .map(|f| format!("{} {}", f.name, f.ty))
            .collect::<Vec<_>>()
            .join("; ");
        let name = format!("struct{{{inner}}}");
        self.anon_structs.entry(name.clone()).or_insert(fields);
        Ok(name)
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

    /// Consume a `[ … ]` bracket group (type parameters `[T any, U comparable]`
    /// or type arguments `[int, string]`) and discard it — generics are erased.
    /// The opening `[` must be the current token.
    /// The token immediately after the `[ … ]` bracket group that starts at the
    /// current position (which must be `[`), or `None` if unbalanced. Used to
    /// tell a generic instantiation (`f[T](` / `T[U]{`) from an index (`xs[i]`).
    fn bracket_group_next(&self) -> Option<&Tok> {
        let mut i = self.pos;
        let mut depth = 0;
        loop {
            match self.tokens.get(i).map(|t| &t.kind)? {
                Tok::LBracket => depth += 1,
                Tok::RBracket => {
                    depth -= 1;
                    if depth == 0 {
                        return self.tokens.get(i + 1).map(|t| &t.kind);
                    }
                }
                Tok::Eof => return None,
                _ => {}
            }
            i += 1;
        }
    }

    fn skip_type_brackets(&mut self) -> Result<(), String> {
        self.expect(&Tok::LBracket)?;
        let mut depth = 1;
        while depth > 0 {
            match self.advance() {
                Tok::LBracket => depth += 1,
                Tok::RBracket => depth -= 1,
                Tok::Eof => return Err("go-rs: unterminated `[` in type parameters".to_string()),
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
        // Erase a generic type-parameter list: `func F[T any](…)`.
        if matches!(self.peek(), Tok::LBracket) {
            self.skip_type_brackets()?;
        }
        self.expect(&Tok::LParen)?;
        let (params, variadic) = self.params()?;
        self.expect(&Tok::RParen)?;
        let (result_names, results): (Vec<String>, Vec<String>) =
            self.results()?.into_iter().unzip();
        let body = self.block()?;
        Ok(Func {
            name,
            receiver,
            params,
            variadic,
            results,
            result_names,
            body,
            line,
        })
    }

    /// Parse a parameter list. Supports grouped parameters that share a type
    /// (`a, b int`): names without a trailing type inherit the next typed
    /// group's type, matching Go's rule.
    fn params(&mut self) -> Result<(Vec<Param>, bool), String> {
        let mut params = Vec::new();
        let mut variadic = false;
        if matches!(self.peek(), Tok::RParen) {
            return Ok((params, variadic));
        }
        // Collect (name, optional-type) entries, then back-fill inherited types.
        let mut pending: Vec<(String, Option<String>)> = Vec::new();
        loop {
            let name = self.ident()?;
            // `name ...T` — a variadic parameter; `ty` is the element type.
            let ty = if matches!(self.peek(), Tok::Ellipsis) {
                self.advance();
                variadic = true;
                Some(self.type_name()?)
            } else if self.type_starts() {
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
        Ok((params, variadic))
    }

    /// Parse a function result signature: nothing, a single type, or a
    /// parenthesized list. Each result is `(name, type)`; `name` is `""` for an
    /// unnamed result. A parenthesized list is *named* iff any identifier in it is
    /// immediately followed by a type (Go requires all-or-none).
    fn results(&mut self) -> Result<Vec<(String, String)>, String> {
        if matches!(self.peek(), Tok::LBrace) {
            return Ok(Vec::new());
        }
        if self.eat(&Tok::LParen) {
            if self.paren_list_is_named() {
                // Named results share the grammar of a parameter list.
                let (params, _) = self.params()?;
                self.expect(&Tok::RParen)?;
                return Ok(params.into_iter().map(|p| (p.name, p.ty)).collect());
            }
            let mut out = Vec::new();
            while !matches!(self.peek(), Tok::RParen) {
                out.push((String::new(), self.type_name()?));
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
            self.expect(&Tok::RParen)?;
            Ok(out)
        } else {
            Ok(vec![(String::new(), self.type_name()?)])
        }
    }

    /// Whether the parenthesized list starting at the current position (just after
    /// its `(`) is a *named* list: some `Ident` is immediately followed by a token
    /// that begins a type (so the ident is a name, not a type).
    fn paren_list_is_named(&self) -> bool {
        let mut i = self.pos;
        let mut depth = 1;
        while i < self.tokens.len() {
            match &self.tokens[i].kind {
                Tok::LParen | Tok::LBracket => depth += 1,
                Tok::RParen if depth == 1 => break,
                Tok::RParen | Tok::RBracket => depth -= 1,
                Tok::Ident(_) if depth == 1 && self.type_starts_at(i + 1) => return true,
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// True if the current token can begin a type in a parameter/result position.
    fn type_starts(&self) -> bool {
        self.type_starts_at(self.pos)
    }

    fn type_starts_at(&self, pos: usize) -> bool {
        matches!(
            self.tokens.get(pos).map(|t| &t.kind),
            Some(Tok::Ident(_))
                | Some(Tok::LBracket)
                | Some(Tok::Star)
                | Some(Tok::Chan)
                | Some(Tok::Func)
        )
    }

    /// Parse a type name. Handles named types (`int`, `string`, `T`, …), slices
    /// (`[]T`), maps (`map[K]V`), and pointers (`*T`) as opaque strings for later
    /// typing.
    fn type_name(&mut self) -> Result<String, String> {
        match self.peek() {
            Tok::LBracket => {
                self.advance();
                // A fixed-size array type `[N]T` or `[...]T` — go-rs represents
                // arrays as slices for the value model, so the size is consumed
                // and the type erases to `[]T`.
                if !matches!(self.peek(), Tok::RBracket) {
                    if matches!(self.peek(), Tok::Ellipsis) {
                        self.advance();
                    } else {
                        let _ = self.expr()?;
                    }
                }
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
            // `struct{ … }` — an anonymous struct type (a field type, a
            // `map[K]struct{…}` value, a `chan struct{}` element, …).
            Tok::Struct => self.anon_struct_type(),
            // `func(params) results` — a function type (for function-typed
            // parameters/fields). Consumed structurally; go-rs treats every
            // function value uniformly, so only the `func` tag is retained.
            Tok::Func => {
                self.advance();
                self.expect(&Tok::LParen)?;
                self.skip_balanced_parens()?;
                // Optional results: a single type or a `( … )` list, ending
                // before the next `,`/`)`/`{`/`;`.
                if matches!(self.peek(), Tok::LParen) {
                    self.advance();
                    self.skip_balanced_parens()?;
                } else if self.type_starts() {
                    let _ = self.type_name()?;
                }
                Ok("func".to_string())
            }
            Tok::Ident(_) => {
                let mut name = self.ident()?;
                // A qualified type from an imported package: `pkg.Type`. The
                // linker rewrites the reference to the merged `pkg.Type` name.
                if matches!(self.peek(), Tok::Dot) {
                    self.advance();
                    let field = self.ident()?;
                    name = format!("{name}.{field}");
                }
                // Erase type arguments on a generic type reference: the base name
                // of `Stack[int]` / `Pair[K, V]` is what go-rs types against.
                if matches!(self.peek(), Tok::LBracket)
                    && (self.generic_names.contains(&name) || name.contains('.'))
                {
                    self.skip_type_brackets()?;
                }
                Ok(name)
            }
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
            Tok::Const => self.const_stmt(),
            Tok::Return => {
                let line = self.line();
                self.advance();
                let mut vals = Vec::new();
                if !matches!(self.peek(), Tok::Semi | Tok::RBrace) {
                    vals.push(self.expr()?);
                    while self.eat(&Tok::Comma) {
                        vals.push(self.expr()?);
                    }
                }
                Ok(Stmt::Return(vals, line))
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
            Tok::Fallthrough => {
                let line = self.line();
                self.advance();
                Ok(Stmt::Fallthrough(line))
            }
            Tok::If => self.if_stmt(),
            Tok::For => self.for_stmt(),
            Tok::Go => {
                let line = self.line();
                self.advance();
                let call = self.expr()?;
                Ok(Stmt::Go { call, line })
            }
            Tok::Defer => {
                let line = self.line();
                self.advance();
                let call = self.expr()?;
                Ok(Stmt::Defer { call, line })
            }
            Tok::Select => self.select_stmt(),
            Tok::Switch => self.switch_stmt(),
            Tok::LBrace => Ok(Stmt::Block(self.block()?)),
            _ => self.simple_stmt(),
        }
    }

    fn var_stmt(&mut self) -> Result<Stmt, String> {
        let line = self.line();
        self.expect(&Tok::Var)?;
        // Grouped form `var ( … )` → a block of single declarations.
        if self.eat(&Tok::LParen) {
            self.skip_semis();
            let mut decls = Vec::new();
            while !matches!(self.peek(), Tok::RParen | Tok::Eof) {
                decls.push(self.var_spec(line)?);
                self.skip_semis();
            }
            self.expect(&Tok::RParen)?;
            return Ok(Stmt::Block(decls));
        }
        self.var_spec(line)
    }

    /// One `name [type] [= init]` variable specification.
    fn var_spec(&mut self, line: u32) -> Result<Stmt, String> {
        let name = self.ident()?;
        // A fixed-size array type `[N]T` is captured specially so a bare
        // `var x [N]T` (no initializer) can be zero-filled to N elements — go-rs
        // models arrays as slices, and the erased size would otherwise be lost.
        let mut array_len: Option<usize> = None;
        let ty = if self.type_starts() && !matches!(self.peek(), Tok::Assign) {
            if matches!(self.peek(), Tok::LBracket) && !matches!(self.peek2(), Tok::RBracket) {
                self.advance(); // `[`
                array_len = if matches!(self.peek(), Tok::Ellipsis) {
                    self.advance();
                    None
                } else {
                    const_int_of(&self.expr()?).map(|n| n as usize)
                };
                self.expect(&Tok::RBracket)?;
                let elem = self.type_name()?;
                Some(format!("[]{elem}"))
            } else {
                Some(self.type_name()?)
            }
        } else {
            None
        };
        let init = if self.eat(&Tok::Assign) {
            Some(self.expr()?)
        } else if let (Some(len), Some(t)) = (array_len, ty.as_ref()) {
            // Bare `var x [N]T` → an N-element slice of the element zero value.
            let elem = t.strip_prefix("[]").unwrap_or(t).to_string();
            let zero = if self.struct_names.contains(&elem) {
                Expr::StructLit {
                    type_name: elem,
                    fields: Vec::new(),
                }
            } else {
                zero_expr(&elem)
            };
            Some(Expr::Make {
                is_map: false,
                len: Some(Box::new(Expr::Int(len as i64))),
                elem_zero: Box::new(zero),
            })
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

    /// `const NAME [type] = expr` or a grouped `const ( … )` block with `iota`.
    /// A constant is modeled as an (immutable-by-convention) `var`; `iota` is the
    /// spec's index within the block, substituted into the expression, and a spec
    /// with no `= …` inherits the previous spec's expression list.
    fn const_stmt(&mut self) -> Result<Stmt, String> {
        let line = self.line();
        self.expect(&Tok::Const)?;
        if self.eat(&Tok::LParen) {
            self.skip_semis();
            let mut decls = Vec::new();
            let mut prev: Option<(Option<String>, Vec<Expr>)> = None;
            let mut idx: i64 = 0;
            while !matches!(self.peek(), Tok::RParen | Tok::Eof) {
                let names = self.ident_list()?;
                let ty = if self.type_starts() && !matches!(self.peek(), Tok::Assign) {
                    Some(self.type_name()?)
                } else {
                    None
                };
                let (ty, exprs) = if self.eat(&Tok::Assign) {
                    let mut es = vec![self.expr()?];
                    while self.eat(&Tok::Comma) {
                        es.push(self.expr()?);
                    }
                    (ty, es)
                } else {
                    // Inherit the previous spec's type + expressions (Go's `iota`
                    // repetition rule); `iota` is re-substituted with this index.
                    match &prev {
                        Some((pty, pes)) => (pty.clone(), pes.clone()),
                        None => return Err(format!("go-rs: missing const value on line {line}")),
                    }
                };
                for (name, expr) in names.iter().zip(&exprs) {
                    decls.push(Stmt::Var {
                        name: name.clone(),
                        ty: ty.clone(),
                        init: Some(subst_iota(expr.clone(), idx)),
                        line,
                    });
                }
                prev = Some((ty, exprs));
                idx += 1;
                self.skip_semis();
            }
            self.expect(&Tok::RParen)?;
            return Ok(Stmt::Block(decls));
        }
        // Single const: `const NAME [type] = expr` (iota == 0).
        let name = self.ident()?;
        let ty = if self.type_starts() && !matches!(self.peek(), Tok::Assign) {
            Some(self.type_name()?)
        } else {
            None
        };
        self.expect(&Tok::Assign)?;
        let init = subst_iota(self.expr()?, 0);
        Ok(Stmt::Var {
            name,
            ty,
            init: Some(init),
            line,
        })
    }

    /// Parse `ident {, ident}` — a comma-separated name list.
    fn ident_list(&mut self) -> Result<Vec<String>, String> {
        let mut names = vec![self.ident()?];
        while self.eat(&Tok::Comma) {
            names.push(self.ident()?);
        }
        Ok(names)
    }

    fn if_stmt(&mut self) -> Result<Stmt, String> {
        let line = self.line();
        self.expect(&Tok::If)?;
        // A `{` in the header starts the body, so suppress bare composite
        // literals while parsing the init/condition.
        let saved = self.no_composite;
        self.no_composite = true;
        // Optional init clause: `if x := f(); cond { … }`.
        let mut init = None;
        let first = self.simple_stmt()?;
        let cond = if self.eat(&Tok::Semi) {
            init = Some(Box::new(first));
            self.expr()?
        } else {
            stmt_into_expr(first, line)?
        };
        self.no_composite = saved;
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

        // Otherwise a condition-only or three-clause header. Suppress bare
        // composite literals while parsing the header (a `{` starts the body).
        let saved = self.no_composite;
        self.no_composite = true;
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
            self.no_composite = saved;
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
            self.no_composite = saved;
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
        // The `{` after the range expression opens the body, so suppress bare
        // composite literals while parsing the iterated expression.
        let saved = self.no_composite;
        self.no_composite = true;
        let iter = self.expr()?;
        self.no_composite = saved;
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

    /// Parse a `select { case …: …; default: … }` statement.
    fn select_stmt(&mut self) -> Result<Stmt, String> {
        let line = self.line();
        self.expect(&Tok::Select)?;
        self.expect(&Tok::LBrace)?;
        self.skip_semis();
        let mut cases = Vec::new();
        let mut default = None;
        while !matches!(self.peek(), Tok::RBrace | Tok::Eof) {
            let ident_kw = matches!(self.peek(), Tok::Ident(s) if s == "default");
            if ident_kw {
                self.advance();
                self.expect(&Tok::Colon)?;
                default = Some(self.case_body()?);
                continue;
            }
            // `case COMM:`
            if !matches!(self.peek(), Tok::Ident(s) if s == "case") {
                return Err(format!(
                    "go-rs: expected `case` or `default` in select on line {}",
                    self.line()
                ));
            }
            self.advance(); // `case`
            let comm = self.select_comm()?;
            self.expect(&Tok::Colon)?;
            let body = self.case_body()?;
            cases.push(SelectClause { comm, body });
        }
        self.expect(&Tok::RBrace)?;
        Ok(Stmt::Select {
            cases,
            default,
            line,
        })
    }

    fn switch_stmt(&mut self) -> Result<Stmt, String> {
        let line = self.line();
        self.expect(&Tok::Switch)?;

        // Optional `init;` then the guard: a tag expression, or a type-switch
        // guard `[v :=] x.(type)`. A `{` in the header opens the case block, so
        // suppress bare composite literals while parsing it.
        let saved = self.no_composite;
        self.no_composite = true;
        let mut init = None;
        let mut guard: Option<Stmt> = None;
        if !matches!(self.peek(), Tok::LBrace) {
            let first = self.simple_stmt()?;
            if self.eat(&Tok::Semi) {
                init = Some(Box::new(first));
                if !matches!(self.peek(), Tok::LBrace) {
                    guard = Some(self.simple_stmt()?);
                }
            } else {
                guard = Some(first);
            }
        }
        self.no_composite = saved;

        // A type switch: the guard is `x.(type)` or `v := x.(type)`.
        if let Some((bind, expr)) = guard.as_ref().and_then(type_switch_guard) {
            return self.type_switch_body(init, bind, expr, line);
        }
        let tag = match guard {
            Some(s) => Some(stmt_into_expr(s, line)?),
            None => None,
        };

        self.expect(&Tok::LBrace)?;
        self.skip_semis();
        let mut cases = Vec::new();
        let mut default = None;
        while !matches!(self.peek(), Tok::RBrace | Tok::Eof) {
            if matches!(self.peek(), Tok::Ident(s) if s == "default") {
                self.advance();
                self.expect(&Tok::Colon)?;
                default = Some(self.case_body()?);
                continue;
            }
            if !matches!(self.peek(), Tok::Ident(s) if s == "case") {
                return Err(format!(
                    "go-rs: expected `case` or `default` in switch on line {}",
                    self.line()
                ));
            }
            self.advance(); // `case`
            let mut exprs = vec![self.expr()?];
            while self.eat(&Tok::Comma) {
                exprs.push(self.expr()?);
            }
            self.expect(&Tok::Colon)?;
            let body = self.case_body()?;
            cases.push(SwitchCase { exprs, body });
        }
        self.expect(&Tok::RBrace)?;
        Ok(Stmt::Switch {
            init,
            tag,
            cases,
            default,
            line,
        })
    }

    /// Parse the body of a type switch (its guard already recognized): `{ case
    /// T1, T2: … ; default: … }`, where each case names types.
    fn type_switch_body(
        &mut self,
        init: Option<Box<Stmt>>,
        bind: Option<String>,
        expr: Expr,
        line: u32,
    ) -> Result<Stmt, String> {
        self.expect(&Tok::LBrace)?;
        self.skip_semis();
        let mut cases = Vec::new();
        let mut default = None;
        while !matches!(self.peek(), Tok::RBrace | Tok::Eof) {
            if matches!(self.peek(), Tok::Ident(s) if s == "default") {
                self.advance();
                self.expect(&Tok::Colon)?;
                default = Some(self.case_body()?);
                continue;
            }
            if !matches!(self.peek(), Tok::Ident(s) if s == "case") {
                return Err(format!(
                    "go-rs: expected `case` or `default` in type switch on line {}",
                    self.line()
                ));
            }
            self.advance(); // `case`
            let mut types = vec![self.type_name()?];
            while self.eat(&Tok::Comma) {
                types.push(self.type_name()?);
            }
            self.expect(&Tok::Colon)?;
            let body = self.case_body()?;
            cases.push(TypeSwitchCase { types, body });
        }
        self.expect(&Tok::RBrace)?;
        Ok(Stmt::TypeSwitch {
            init,
            bind,
            expr,
            cases,
            default,
            line,
        })
    }

    /// Parse the communication clause of a select case (before the `:`).
    fn select_comm(&mut self) -> Result<SelectComm, String> {
        // `<-ch` (receive, no bind).
        if matches!(self.peek(), Tok::Arrow) {
            self.advance();
            return Ok(SelectComm::Recv {
                bind: None,
                chan: self.expr()?,
            });
        }
        let first = self.expr()?;
        match self.peek() {
            // `v := <-ch` / `v = <-ch` (receive with bind).
            Tok::Define | Tok::Assign => {
                self.advance();
                self.expect(&Tok::Arrow)?;
                let chan = self.expr()?;
                let bind = match first {
                    Expr::Ident(n) if n != "_" => Some(n),
                    _ => None,
                };
                Ok(SelectComm::Recv { bind, chan })
            }
            // `ch <- val` (send).
            Tok::Arrow => {
                self.advance();
                Ok(SelectComm::Send {
                    chan: first,
                    val: self.expr()?,
                })
            }
            other => Err(format!(
                "go-rs: expected a channel operation in select case, found `{other}`"
            )),
        }
    }

    /// Parse the statements of a select case/default body (up to the next
    /// `case`/`default`/`}`).
    fn case_body(&mut self) -> Result<Vec<Stmt>, String> {
        self.skip_semis();
        let mut body = Vec::new();
        while !matches!(self.peek(), Tok::RBrace | Tok::Eof)
            && !matches!(self.peek(), Tok::Ident(s) if s == "case" || s == "default")
        {
            body.push(self.stmt()?);
            self.skip_semis();
        }
        Ok(body)
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
        // Parallel assignment `t0, t1, … = v0, v1, …`.
        if matches!(self.peek(), Tok::Comma) {
            let mut targets = vec![target];
            while self.eat(&Tok::Comma) {
                targets.push(self.expr()?);
            }
            self.expect(&Tok::Assign)?;
            let mut values = vec![self.expr()?];
            while self.eat(&Tok::Comma) {
                values.push(self.expr()?);
            }
            return Ok(Stmt::AssignMulti {
                targets,
                values,
                line,
            });
        }
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
            | Tok::PercentAssign
            | Tok::AmpAssign
            | Tok::PipeAssign
            | Tok::CaretAssign
            | Tok::ShlAssign
            | Tok::ShrAssign
            | Tok::AndNotAssign => {
                let op = match self.advance() {
                    Tok::Assign => AssignOp::Set,
                    Tok::PlusAssign => AssignOp::Add,
                    Tok::MinusAssign => AssignOp::Sub,
                    Tok::StarAssign => AssignOp::Mul,
                    Tok::SlashAssign => AssignOp::Div,
                    Tok::PercentAssign => AssignOp::Mod,
                    Tok::AmpAssign => AssignOp::BitAnd,
                    Tok::PipeAssign => AssignOp::BitOr,
                    Tok::CaretAssign => AssignOp::BitXor,
                    Tok::ShlAssign => AssignOp::Shl,
                    Tok::ShrAssign => AssignOp::Shr,
                    Tok::AndNotAssign => AssignOp::AndNot,
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
            // `&x` — address-of (a no-copy reference on go-rs handles).
            Tok::Amp => {
                self.advance();
                Ok(Expr::Unary {
                    op: UnOp::Addr,
                    rhs: Box::new(self.unary()?),
                })
            }
            // `^x` — bitwise complement.
            Tok::Caret => {
                self.advance();
                Ok(Expr::Unary {
                    op: UnOp::BitNot,
                    rhs: Box::new(self.unary()?),
                })
            }
            // `*p` — pointer dereference (identity on a go-rs handle).
            Tok::Star => {
                self.advance();
                Ok(Expr::Unary {
                    op: UnOp::Deref,
                    rhs: Box::new(self.unary()?),
                })
            }
            _ => self.postfix(),
        }
    }

    fn postfix(&mut self) -> Result<Expr, String> {
        let mut e = self.primary()?;
        loop {
            // Erase type arguments on a generic instantiation: `F[int](x)` and
            // `Stack[int]{…}`. When the base is a known generic name, the `[ … ]`
            // is type arguments (not an index), so consume and drop it, leaving
            // the bare name to be called or composite-literal'd below.
            if matches!(self.peek(), Tok::LBracket) {
                if let Expr::Ident(n) = &e {
                    if self.generic_names.contains(n) {
                        let base = n.clone();
                        self.skip_type_brackets()?;
                        // `Stack[int]{ … }` — a generic struct composite literal.
                        if matches!(self.peek(), Tok::LBrace) && self.struct_names.contains(&base) {
                            e = self.struct_literal(base)?;
                        }
                        continue;
                    }
                }
                // A package-qualified base with a bracket group immediately
                // followed by `(` or `{` is a generic instantiation imported from
                // another package — `pkg.Func[T](…)` / `pkg.Type[T]{…}` — which the
                // local `generic_names` set can't know. Erase the type arguments.
                // Restricted to a `pkg.Name` selector base so an index-then-call
                // `fns[i](x)` (plain-ident base) is unaffected.
                if matches!(&e, Expr::Selector { recv, .. } if matches!(recv.as_ref(), Expr::Ident(_)))
                {
                    let next = self.bracket_group_next();
                    // `pkg.Func[T](…)` — a generic call. The `(` is never a loop
                    // body, so this is unambiguous.
                    if matches!(next, Some(Tok::LParen)) {
                        self.skip_type_brackets()?;
                        continue;
                    }
                    // `pkg.Type[T]{ … }` — a generic composite. Ambiguous with an
                    // index in a control header (`range m[k] {`), so gated.
                    if matches!(next, Some(Tok::LBrace)) && !self.no_composite {
                        self.skip_type_brackets()?;
                        if let Expr::Selector { recv, field } = &e {
                            if let Expr::Ident(pkg) = recv.as_ref() {
                                e = self.struct_literal(format!("{pkg}.{field}"))?;
                            }
                        }
                        continue;
                    }
                }
            }
            match self.peek() {
                Tok::Dot => {
                    self.advance();
                    // Type assertion `x.(T)` / type-switch guard `x.(type)`.
                    if self.eat(&Tok::LParen) {
                        let ty = if self.eat(&Tok::Type) {
                            "type".to_string()
                        } else {
                            self.type_name()?
                        };
                        self.expect(&Tok::RParen)?;
                        e = Expr::TypeAssert {
                            expr: Box::new(e),
                            ty,
                        };
                    } else {
                        let field = self.ident()?;
                        // `pkg.Type{ … }` — a composite literal of a type imported
                        // from another package. Recognized when the receiver is a
                        // bare package identifier and a `{` follows (outside a
                        // control-clause header). The linker qualifies `pkg.Type`
                        // to the merged struct name the compiler then resolves.
                        if !self.no_composite
                            && matches!(self.peek(), Tok::LBrace)
                            && matches!(&e, Expr::Ident(_))
                        {
                            let Expr::Ident(pkg) = &e else { unreachable!() };
                            e = self.struct_literal(format!("{pkg}.{field}"))?;
                        } else {
                            e = Expr::Selector {
                                recv: Box::new(e),
                                field,
                            };
                        }
                    }
                }
                Tok::LBracket => {
                    self.advance();
                    // `recv[low:high]` slice expression, or `recv[index]`. Either
                    // bound of a slice may be omitted (`s[:hi]`, `s[lo:]`, `s[:]`).
                    let low = if matches!(self.peek(), Tok::Colon) {
                        None
                    } else {
                        Some(self.expr()?)
                    };
                    if self.eat(&Tok::Colon) {
                        let high = if matches!(self.peek(), Tok::RBracket | Tok::Colon) {
                            None
                        } else {
                            Some(self.expr()?)
                        };
                        // A three-index (full) slice `s[low:high:max]` — the
                        // capacity bound is parsed and ignored (go-rs sub-slices
                        // copy, so capacity has no effect).
                        if self.eat(&Tok::Colon) {
                            let _ = self.expr()?;
                        }
                        self.expect(&Tok::RBracket)?;
                        e = Expr::Slice {
                            recv: Box::new(e),
                            low: low.map(Box::new),
                            high: high.map(Box::new),
                        };
                    } else {
                        self.expect(&Tok::RBracket)?;
                        e = Expr::Index {
                            recv: Box::new(e),
                            index: Box::new(low.expect("index expr present")),
                        };
                    }
                }
                Tok::LParen => {
                    let line = self.line();
                    self.advance();
                    // Call-argument parentheses reset composite suppression.
                    let saved = self.no_composite;
                    self.no_composite = false;
                    let mut args = Vec::new();
                    let mut spread = false;
                    while !matches!(self.peek(), Tok::RParen) {
                        args.push(self.expr()?);
                        // `f(xs...)` — spread the last argument into a variadic call.
                        if matches!(self.peek(), Tok::Ellipsis) {
                            self.advance();
                            spread = true;
                        }
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    self.expect(&Tok::RParen)?;
                    self.no_composite = saved;
                    e = Expr::Call {
                        func: Box::new(e),
                        args,
                        spread,
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
        // `[]T{ … }` slice literal / `[]T(x)` conversion, or a fixed-size array
        // literal `[N]T{ … }` / `[...]T{ … }`.
        if matches!(self.peek(), Tok::LBracket) {
            // Peek past `[` to distinguish a slice (`[]`) from an array (`[N]`
            // or `[...]`).
            if matches!(self.peek2(), Tok::RBracket) {
                return self.slice_literal();
            }
            return self.array_literal();
        }
        // `struct{ … }{ … }` — an anonymous struct composite literal.
        if matches!(self.peek(), Tok::Struct) {
            let ty = self.anon_struct_type()?;
            return self.struct_literal(ty);
        }
        match self.advance() {
            Tok::Int(n) => Ok(Expr::Int(n)),
            Tok::Float(f, dec) => Ok(Expr::Float(f, dec)),
            Tok::Str(s) => Ok(Expr::Str(s)),
            Tok::True => Ok(Expr::Bool(true)),
            Tok::False => Ok(Expr::Bool(false)),
            Tok::Ident(s) => {
                match s.as_str() {
                    // `map[K]V{ … }` map composite literal.
                    "map" if matches!(self.peek(), Tok::LBracket) => self.map_literal(),
                    // `make([]T, n)` / `make(map[K]V)`.
                    "make" if matches!(self.peek(), Tok::LParen) => self.make_expr(),
                    // `new(T)` — a pointer to a zero value of T. Faithful lowering:
                    // `&T{}` for a struct (the compiler zero-fills missing fields),
                    // and address-of the scalar zero value otherwise.
                    "new" if matches!(self.peek(), Tok::LParen) => self.new_expr(),
                    // `T{ … }` struct composite literal (T declared as a struct).
                    _ if matches!(self.peek(), Tok::LBrace) && self.struct_names.contains(&s) => {
                        self.struct_literal(s)
                    }
                    _ => Ok(Expr::Ident(s)),
                }
            }
            Tok::LParen => {
                // Parentheses reset the composite-literal suppression: `if
                // (Point{}).x > 0 { … }` is a valid composite inside a header.
                let saved = self.no_composite;
                self.no_composite = false;
                let e = self.expr()?;
                self.expect(&Tok::RParen)?;
                self.no_composite = saved;
                Ok(e)
            }
            // A function literal (closure): `func(params) results { body }`.
            Tok::Func => {
                self.expect(&Tok::LParen)?;
                // Variadic closures are uncommon; the flag is dropped for FuncLit.
                let (params, _) = self.params()?;
                self.expect(&Tok::RParen)?;
                // Closures keep only result types (named results on a func literal
                // are uncommon; the name is dropped).
                let results = self.results()?.into_iter().map(|(_, t)| t).collect();
                let body = self.block()?;
                Ok(Expr::FuncLit {
                    params,
                    results,
                    body,
                })
            }
            other => Err(format!(
                "go-rs: unexpected token `{other}` in expression on line {line}"
            )),
        }
    }

    /// `[]T{ e0, e1, … }` (slice composite literal) or `[]T(x)` (a slice
    /// conversion — `[]byte(s)` / `[]rune(s)`).
    fn slice_literal(&mut self) -> Result<Expr, String> {
        self.expect(&Tok::LBracket)?;
        self.expect(&Tok::RBracket)?;
        let elem_ty = self.type_name()?;
        // `[]byte(x)` / `[]rune(x)` — a conversion, not a composite literal.
        if matches!(self.peek(), Tok::LParen) {
            let line = self.line();
            self.expect(&Tok::LParen)?;
            let arg = self.expr()?;
            self.expect(&Tok::RParen)?;
            return Ok(Expr::Call {
                func: Box::new(Expr::Ident(format!("[]{elem_ty}"))),
                args: vec![arg],
                spread: false,
                line,
            });
        }
        self.expect(&Tok::LBrace)?;
        let mut elems = Vec::new();
        while !matches!(self.peek(), Tok::RBrace) {
            // Elided element type: `[]T{ {…}, {…} }` — a bare `{…}` is a
            // composite literal of the slice's element type. This applies to a
            // locally-known struct and to a qualified imported type (`pkg.T`,
            // which the parser can't see is a struct); container element types
            // (`[]U`, `map[…]`) keep their own literal syntax.
            let elided_struct = matches!(self.peek(), Tok::LBrace)
                && (self.struct_names.contains(&elem_ty)
                    || elem_ty.starts_with("struct{")
                    || (elem_ty.contains('.')
                        && !elem_ty.starts_with("[]")
                        && !elem_ty.starts_with("map[")));
            if elided_struct {
                elems.push(self.struct_literal(elem_ty.clone())?);
            } else {
                elems.push(self.expr()?);
            }
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(&Tok::RBrace)?;
        Ok(Expr::SliceLit { elem_ty, elems })
    }

    /// `[N]T{ … }` / `[...]T{ … }` fixed-size array literal. go-rs represents an
    /// array as a slice, so this builds a `SliceLit` of the array's length.
    /// Elements may be sequential (`v0, v1, …`) or index-keyed (`3: v`), the
    /// latter zero-filling any gaps; a `[...]` array is sized by its elements.
    fn array_literal(&mut self) -> Result<Expr, String> {
        self.expect(&Tok::LBracket)?;
        // Size: `...` (element-sized) or a constant expression `[N]`.
        let fixed_len: Option<usize> = if matches!(self.peek(), Tok::Ellipsis) {
            self.advance();
            None
        } else {
            let n = self.expr()?;
            const_int_of(&n).map(|n| n as usize)
        };
        self.expect(&Tok::RBracket)?;
        let elem_ty = self.type_name()?;
        self.expect(&Tok::LBrace)?;
        // Collect (index, value) placements; a sequential value takes the running
        // index, an index-keyed value resets it.
        let mut placed: Vec<(usize, Expr)> = Vec::new();
        let mut next_idx = 0usize;
        while !matches!(self.peek(), Tok::RBrace) {
            // A bare `{ … }` is an elided composite of a struct element type
            // (sequential, no index key).
            let is_bare_struct =
                matches!(self.peek(), Tok::LBrace) && self.struct_names.contains(&elem_ty);
            if is_bare_struct {
                let v = self.struct_literal(elem_ty.clone())?;
                placed.push((next_idx, v));
                next_idx += 1;
            } else {
                let first = self.expr()?;
                if self.eat(&Tok::Colon) {
                    // `idx: value` — an index-keyed element.
                    let idx = const_int_of(&first).ok_or_else(|| {
                        format!(
                            "go-rs: array index in a composite literal must be a constant (line {})",
                            self.line()
                        )
                    })? as usize;
                    let v = if matches!(self.peek(), Tok::LBrace)
                        && self.struct_names.contains(&elem_ty)
                    {
                        self.struct_literal(elem_ty.clone())?
                    } else {
                        self.expr()?
                    };
                    placed.push((idx, v));
                    next_idx = idx + 1;
                } else {
                    placed.push((next_idx, first));
                    next_idx += 1;
                }
            }
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(&Tok::RBrace)?;
        // Final length: the declared `[N]`, else one past the highest index.
        let len = fixed_len.unwrap_or_else(|| placed.iter().map(|(i, _)| i + 1).max().unwrap_or(0));
        // Zero value for any gap: an empty struct literal for a struct element
        // type (the compiler zero-fills), else the scalar zero.
        let zero = |ety: &str| -> Expr {
            if self.struct_names.contains(ety) {
                Expr::StructLit {
                    type_name: ety.to_string(),
                    fields: Vec::new(),
                }
            } else {
                zero_expr(ety)
            }
        };
        let mut elems: Vec<Expr> = (0..len).map(|_| zero(&elem_ty)).collect();
        for (idx, v) in placed {
            if idx < elems.len() {
                elems[idx] = v;
            }
        }
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
        // Inside the composite's braces the `{`-ambiguity is over, so nested
        // composites in field values are allowed again.
        let saved = self.no_composite;
        self.no_composite = false;
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
        self.no_composite = saved;
        Ok(Expr::StructLit { type_name, fields })
    }

    /// `new(T)` — allocate a zero value of `T` and return a pointer to it. A
    /// struct `T` lowers to `&T{}` (the compiler zero-fills missing fields); a
    /// scalar `T` lowers to the address of its zero value.
    fn new_expr(&mut self) -> Result<Expr, String> {
        self.expect(&Tok::LParen)?;
        let ty = self.type_name()?;
        self.expect(&Tok::RParen)?;
        let inner = if self.struct_names.contains(&ty) {
            Expr::StructLit {
                type_name: ty,
                fields: Vec::new(),
            }
        } else {
            zero_expr(&ty)
        };
        Ok(Expr::Unary {
            op: crate::ast::UnOp::Addr,
            rhs: Box::new(inner),
        })
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

/// The constant integer value of an expression, if it folds to one — used for
/// array sizes and index-keyed array-literal elements. Handles literals and the
/// simple unary/binary arithmetic that appears in array bounds.
fn const_int_of(e: &Expr) -> Option<i64> {
    match e {
        Expr::Int(n) => Some(*n),
        Expr::Unary {
            op: crate::ast::UnOp::Neg,
            rhs,
        } => const_int_of(rhs).map(|n| -n),
        Expr::Binary { op, lhs, rhs } => {
            use crate::ast::BinOp;
            let (a, b) = (const_int_of(lhs)?, const_int_of(rhs)?);
            match op {
                BinOp::Add => Some(a + b),
                BinOp::Sub => Some(a - b),
                BinOp::Mul => Some(a * b),
                BinOp::Div if b != 0 => Some(a / b),
                BinOp::Mod if b != 0 => Some(a % b),
                BinOp::Shl => Some(a << b),
                BinOp::Shr => Some(a >> b),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The zero-value expression for a Go element type (drives `make([]T, n)` fill).
fn zero_expr(ty: &str) -> Expr {
    match ty {
        "float32" | "float64" => Expr::Float(0.0, Some((0, 0))),
        "string" => Expr::Str(String::new()),
        "bool" => Expr::Bool(false),
        _ => Expr::Int(0),
    }
}

/// The binary operator and its precedence for a token, or `None` if the token
/// is not a binary operator. Higher binds tighter (Go's five levels).
/// Replace every `iota` identifier in a constant expression with its integer
/// value `idx` (the const spec's index within its block).
fn subst_iota(e: Expr, idx: i64) -> Expr {
    let b = |e: Box<Expr>| Box::new(subst_iota(*e, idx));
    match e {
        Expr::Ident(n) if n == "iota" => Expr::Int(idx),
        Expr::Unary { op, rhs } => Expr::Unary { op, rhs: b(rhs) },
        Expr::Binary { op, lhs, rhs } => Expr::Binary {
            op,
            lhs: b(lhs),
            rhs: b(rhs),
        },
        Expr::Call {
            func,
            args,
            spread,
            line,
        } => Expr::Call {
            func: b(func),
            args: args.into_iter().map(|a| subst_iota(a, idx)).collect(),
            spread,
            line,
        },
        Expr::Index { recv, index } => Expr::Index {
            recv: b(recv),
            index: b(index),
        },
        Expr::Selector { recv, field } => Expr::Selector {
            recv: b(recv),
            field,
        },
        other => other,
    }
}

/// If `s` is a type-switch guard (`x.(type)` or `v := x.(type)`), return the
/// optional bound name and the asserted expression.
fn type_switch_guard(s: &Stmt) -> Option<(Option<String>, Expr)> {
    let is_type_assert = |e: &Expr| matches!(e, Expr::TypeAssert { ty, .. } if ty == "type");
    match s {
        Stmt::ExprStmt(e) if is_type_assert(e) => {
            if let Expr::TypeAssert { expr, .. } = e {
                Some((None, (**expr).clone()))
            } else {
                None
            }
        }
        Stmt::Short { names, values, .. } if names.len() == 1 && values.len() == 1 => {
            if let Expr::TypeAssert { expr, ty } = &values[0] {
                if ty == "type" {
                    return Some((Some(names[0].clone()), (**expr).clone()));
                }
            }
            None
        }
        _ => None,
    }
}

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
        // Additive level (4): `+ - | ^`.
        Tok::Plus => (BinOp::Add, 4),
        Tok::Minus => (BinOp::Sub, 4),
        Tok::Pipe => (BinOp::BitOr, 4),
        Tok::Caret => (BinOp::BitXor, 4),
        // Multiplicative level (5): `* / % << >> & &^`.
        Tok::Star => (BinOp::Mul, 5),
        Tok::Slash => (BinOp::Div, 5),
        Tok::Percent => (BinOp::Mod, 5),
        Tok::Shl => (BinOp::Shl, 5),
        Tok::Shr => (BinOp::Shr, 5),
        Tok::Amp => (BinOp::BitAnd, 5),
        Tok::AndNot => (BinOp::AndNot, 5),
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
