//! Multi-package linking: resolve `import` paths to Go **source**, parse each
//! imported package, and merge everything into one compile unit.
//!
//! go-rs is an executor swap — the standard library is real Go code, so an
//! imported package is *run from its source*, not reimplemented. Each package's
//! top-level names are qualified with the import path (`errors.New`,
//! `errors.errorString`) so many packages coexist in one [`crate::ast::Program`] that
//! the existing single-program [`crate::compiler`] then lowers unchanged.
//!
//! A small set of packages ([`NATIVE`]) stay as host builtins: the irreducible
//! runtime/I-O boundary (`fmt` writes to stdout, `os` touches the OS) that can't
//! be expressed in portable Go. Everything else is loaded from source.

use crate::ast::*;
use std::collections::{HashMap, HashSet};

/// Packages provided by native host builtins (the runtime/syscall boundary) —
/// left as package selectors for the compiler, never loaded from source.
pub const NATIVE: &[&str] = &["fmt", "strings", "strconv", "math", "sort", "os"];

/// Names a vendored package references but does not declare: host intrinsics the
/// compiler lowers directly (`errors.asTag` needs the runtime type tag a type
/// switch dispatches on, which no Go source can name). Listed here so they are
/// qualified like the package's own names — a user program's `runtimeTypeTag`
/// then cannot capture the reference.
const INTRINSICS: &[(&str, &str)] = &[("errors", "runtimeTypeTag")];

/// Whether a type name is the parser's canonical name for an anonymous interface
/// (`interface{Unwrap}`), which names a method set rather than a package's type.
fn is_anon_iface(name: &str) -> bool {
    name.starts_with("interface{")
}

/// The default local name of an import path — its last segment
/// (`unicode/utf8` → `utf8`).
fn import_alias(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Resolve, parse, qualify, and merge every (source) package imported by `main`
/// (transitively) into it, returning a single program the compiler can lower.
pub fn link(mut main: Program) -> Result<Program, String> {
    let mut loaded: HashMap<String, Program> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for path in main.imports.clone() {
        load_recursive(&path, &mut loaded, &mut order)?;
    }
    // `main` keeps its own names, but its references to imported source packages
    // (`errors.New` → the qualified `errors.New`) are rewritten — before the
    // (already-qualified) packages are merged in.
    qualify(&mut main, "main", false);
    // Merge loaded packages (dependencies first) ahead of `main`'s own code, so
    // package-level initializers run before `main` does.
    let mut init_globals: Vec<Stmt> = Vec::new();
    for path in &order {
        let pkg = loaded.remove(path).expect("loaded package");
        init_globals.extend(pkg.main); // the package's init-order globals
        main.funcs.extend(pkg.funcs);
        main.types.extend(pkg.types);
        main.interfaces.extend(pkg.interfaces);
    }
    init_globals.extend(std::mem::take(&mut main.main));
    main.main = init_globals;
    // `fmt.Errorf` and the `strconv` conversions build real error values —
    // synthesize their types before `$stringify` so the helper picks up each
    // `Error()` method. The `strconv` sentinels are `errors.New`-shaped, so
    // `NumError` implies the `fmt.Errorf` types too.
    let strconv_errors = uses_strconv_error(&main);
    if uses_errorf(&main) || strconv_errors {
        add_errorf_type(&mut main);
    }
    if strconv_errors {
        add_num_error_type(&mut main);
    }
    // `strings.Builder` is a *type* in a package whose functions are host
    // builtins, so it cannot arrive through the source-package path; it is
    // synthesized and qualified instead. Gated on the import, because a program
    // that never names `strings` cannot name the type either.
    if main.imports.iter().any(|p| p == "strings") {
        add_strings_builder(&mut main)?;
    }
    add_sort_slice(&mut main);
    add_stringify(&mut main);
    Ok(main)
}

/// Synthesize `$sortSlice(s, less)` — the in-language target of `sort.Slice` /
/// `sort.SliceStable`, whose comparator is a VM closure a host builtin can't
/// call. An insertion sort keyed on `less(j, j-1)` (stable; the swap works on
/// go-rs's reference-typed slice, mutating the caller's slice in place).
fn add_sort_slice(prog: &mut Program) {
    let src = "package p\n\
        func h(s any, less func(int, int) bool) {\n\
        \tfor i := 1; i < len(s); i++ {\n\
        \t\tfor j := i; j > 0 && less(j, j-1); j-- {\n\
        \t\t\ts[j], s[j-1] = s[j-1], s[j]\n\
        \t\t}\n\
        \t}\n\
        }\n";
    if let Ok(mut p) = crate::parse(src) {
        if let Some(mut f) = p.funcs.pop() {
            f.name = "$sortSlice".to_string();
            prog.funcs.push(f);
        }
    }
}

/// Whether any expression anywhere in `prog` — `main`'s body, every function
/// body, and every nested statement or sub-expression of either — satisfies
/// `pred`. Drives the "does this program need type X synthesized" questions.
fn program_has_expr(prog: &Program, pred: &dyn Fn(&Expr) -> bool) -> bool {
    fn in_expr(e: &Expr, pred: &dyn Fn(&Expr) -> bool) -> bool {
        if pred(e) {
            return true;
        }
        let sub = |e: &Expr| in_expr(e, pred);
        match e {
            Expr::Call { func, args, .. } => sub(func) || args.iter().any(sub),
            Expr::Selector { recv, .. } => sub(recv),
            Expr::Unary { rhs, .. } => sub(rhs),
            Expr::Binary { lhs, rhs, .. } => sub(lhs) || sub(rhs),
            Expr::Index { recv, index } => sub(recv) || sub(index),
            Expr::Slice {
                recv,
                low,
                high,
                max,
            } => {
                sub(recv)
                    || low.as_deref().is_some_and(sub)
                    || high.as_deref().is_some_and(sub)
                    || max.as_deref().is_some_and(sub)
            }
            Expr::StructLit { fields, .. } => fields.iter().any(|(_, v)| sub(v)),
            Expr::SliceLit { elems, .. } => elems.iter().any(sub),
            Expr::MapLit { pairs, .. } => pairs.iter().any(|(k, v)| sub(k) || sub(v)),
            Expr::FuncLit { body, .. } => body.iter().any(|s| in_stmt(s, pred)),
            _ => false,
        }
    }
    fn in_stmt(s: &Stmt, pred: &dyn Fn(&Expr) -> bool) -> bool {
        let ex = |e: &Expr| in_expr(e, pred);
        let st = |s: &Stmt| in_stmt(s, pred);
        match s {
            Stmt::ExprStmt(e) => ex(e),
            Stmt::Return(es, ..) => es.iter().any(ex),
            Stmt::Var { init, .. } => init.as_ref().is_some_and(ex),
            Stmt::Short { values, .. } => values.iter().any(ex),
            Stmt::Assign { target, value, .. } => ex(target) || ex(value),
            Stmt::AssignMulti {
                targets, values, ..
            } => targets.iter().chain(values).any(ex),
            Stmt::IncDec { target, .. } => ex(target),
            Stmt::If {
                init,
                cond,
                then,
                els,
                ..
            } => {
                init.as_deref().is_some_and(st)
                    || ex(cond)
                    || then.iter().any(st)
                    || els.iter().any(st)
            }
            Stmt::For {
                init,
                cond,
                post,
                body,
                ..
            } => {
                init.as_deref().is_some_and(st)
                    || cond.as_ref().is_some_and(ex)
                    || post.as_deref().is_some_and(st)
                    || body.iter().any(st)
            }
            Stmt::ForRange { iter, body, .. } => ex(iter) || body.iter().any(st),
            Stmt::Block(b) => b.iter().any(st),
            Stmt::Defer { call, .. } | Stmt::Go { call, .. } => ex(call),
            Stmt::Send { chan, val, .. } => ex(chan) || ex(val),
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
                        .any(|c| c.exprs.iter().any(ex) || c.body.iter().any(st))
                    || default.as_ref().is_some_and(|d| d.iter().any(st))
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
                    || cases.iter().any(|c| c.body.iter().any(st))
                    || default.as_ref().is_some_and(|d| d.iter().any(st))
            }
            Stmt::Select { cases, default, .. } => {
                cases.iter().any(|c| c.body.iter().any(st))
                    || default.as_ref().is_some_and(|d| d.iter().any(st))
            }
            _ => false,
        }
    }
    prog.main.iter().any(|s| in_stmt(s, pred))
        || prog
            .funcs
            .iter()
            .any(|f| f.body.iter().any(|s| in_stmt(s, pred)))
}

/// Whether `e` is the selector `pkg.name`.
fn is_selector(e: &Expr, pkg: &str, name: &str) -> bool {
    matches!(e, Expr::Selector { recv, field }
        if field == name && matches!(recv.as_ref(), Expr::Ident(p) if p == pkg))
}

/// Whether the program calls anything that yields a host-built error value —
/// `fmt.Errorf`, or a `strconv` conversion whose second result is one — which
/// drives synthesis of the error types those values use.
fn uses_errorf(prog: &Program) -> bool {
    /// `pkg.Func` calls that build an error value.
    const ERROR_MAKERS: &[(&str, &str)] = &[
        ("fmt", "Errorf"),
        ("strconv", "Atoi"),
        ("strconv", "ParseInt"),
        ("strconv", "ParseFloat"),
    ];
    program_has_expr(prog, &|e| {
        matches!(e, Expr::Call { func, .. }
            if ERROR_MAKERS.iter().any(|(p, f)| is_selector(func, p, f)))
    })
}

/// Whether the program can observe a `*strconv.NumError` — it either runs a
/// conversion that builds one, or names one of the sentinels such a value wraps.
fn uses_strconv_error(prog: &Program) -> bool {
    /// The `strconv` conversions whose second result is a `*NumError`.
    const PARSERS: &[&str] = &["Atoi", "ParseInt", "ParseFloat", "ParseBool"];
    program_has_expr(prog, &|e| match e {
        Expr::Call { func, .. } => PARSERS.iter().any(|f| is_selector(func, "strconv", f)),
        e => is_selector(e, "strconv", "ErrSyntax") || is_selector(e, "strconv", "ErrRange"),
    })
}

/// Synthesize `strings.Builder` and its method set from real Go source, then
/// qualify it under `strings` so the declaration is named exactly what a
/// program writes. See `goroot/strings_builder.go` for why the type cannot come
/// in through [`load_recursive`] like `errors` and `io` do.
fn add_strings_builder(prog: &mut Program) -> Result<(), String> {
    let mut pkg = crate::parse(include_str!("../goroot/strings_builder.go"))?;
    qualify(&mut pkg, "strings", true);
    prog.types.append(&mut pkg.types);
    prog.funcs.append(&mut pkg.funcs);
    Ok(())
}

/// Synthesize the three error types `fmt.Errorf` constructs, mirroring Go's
/// `fmt/errors.go`: a plain message error when the format wraps nothing, a
/// `wrapError` (`Unwrap() error`) for one `%w`, and a `wrapErrors`
/// (`Unwrap() []error`) for several. Each has a message field `s` returned by
/// `Error()`; the compiler picks the type at the `fmt.Errorf` call site from the
/// `%w` verbs in the format.
fn add_errorf_type(prog: &mut Program) {
    /// `func (e *T) name() ret { return e.field }`
    fn getter(ty: &str, name: &str, field: &str, ret: &str) -> Func {
        Func {
            name: name.to_string(),
            receiver: Some(Param {
                name: "e".to_string(),
                ty: format!("*{ty}"),
            }),
            params: vec![],
            variadic: false,
            results: vec![ret.to_string()],
            result_names: vec![String::new()],
            body: vec![Stmt::Return(
                vec![Expr::Selector {
                    recv: Box::new(Expr::Ident("e".to_string())),
                    field: field.to_string(),
                }],
                0,
            )],
            line: 0,
        }
    }
    let msg = Param {
        name: "s".to_string(),
        ty: "string".to_string(),
    };
    for (ty, extra, unwrap_ret) in [
        ("$errorString", None, None),
        ("$wrapError", Some(("err", "error")), Some("error")),
        ("$wrapErrors", Some(("errs", "[]error")), Some("[]error")),
    ] {
        let mut fields = vec![msg.clone()];
        if let Some((name, fty)) = extra {
            fields.push(Param {
                name: name.to_string(),
                ty: fty.to_string(),
            });
        }
        prog.types.push(StructDecl {
            name: ty.to_string(),
            fields,
        });
        prog.funcs.push(getter(ty, "Error", "s", "string"));
        if let (Some((field, _)), Some(ret)) = (extra, unwrap_ret) {
            prog.funcs.push(getter(ty, "Unwrap", field, ret));
        }
    }
}

/// Synthesize `strconv.NumError` — the error type Go's `strconv` conversions
/// return — from Go's own `strconv/atoi.go`:
///
/// ```go
/// type NumError struct { Func, Num string; Err error }
/// func (e *NumError) Error() string {
///     return "strconv." + e.Func + ": " + "parsing " + Quote(e.Num) + ": " + e.Err.Error()
/// }
/// func (e *NumError) Unwrap() error { return e.Err }
/// ```
///
/// `strconv` is a native host package ([`NATIVE`]) because its parsers reach the
/// float runtime, so the one *source-level* type it exports is synthesized here
/// under its linked name instead of loaded from `goroot`. The host builds the
/// value ([`crate::host::stdlib::NUM_ERROR`]); this supplies the two methods, so
/// the message text, `errors.Is` (via `Unwrap`) and `errors.As` (via the type
/// tag) all come off the real type rather than a stand-in.
fn add_num_error_type(prog: &mut Program) {
    let ty = crate::host::stdlib::NUM_ERROR;
    let src = "package p\n\
        type NumError struct {\n\
        \tFunc string\n\
        \tNum  string\n\
        \tErr  error\n\
        }\n\
        func (e *NumError) Error() string {\n\
        \treturn \"strconv.\" + e.Func + \": \" + \"parsing \" + strconv.Quote(e.Num) + \": \" + e.Err.Error()\n\
        }\n\
        func (e *NumError) Unwrap() error {\n\
        \treturn e.Err\n\
        }\n";
    let Ok(mut p) = crate::parse(src) else { return };
    for t in &mut p.types {
        t.name = ty.to_string();
    }
    for f in &mut p.funcs {
        if let Some(r) = &mut f.receiver {
            r.ty = format!("*{ty}");
        }
    }
    prog.types.append(&mut p.types);
    prog.funcs.append(&mut p.funcs);
}

/// Synthesize the `$stringify` helper — a type switch over every type with a
/// zero-argument `Error()`/`String()` method that calls it — so `fmt` prints
/// error and `Stringer` values via their method (Go's fmt interface handling).
/// The compiler wraps each `fmt.Print*`/`Sprint*` argument with a call to it.
fn add_stringify(prog: &mut Program) {
    // type → the method fmt should call (Error preferred over String).
    let mut chosen: HashMap<String, &str> = HashMap::new();
    for method in ["String", "Error"] {
        for f in &prog.funcs {
            if let Some(r) = &f.receiver {
                if f.name == method && f.params.is_empty() {
                    let ty = r.ty.trim_start_matches('*').to_string();
                    chosen.insert(ty, method); // Error's later pass wins
                }
            }
        }
    }
    if chosen.is_empty() {
        return;
    }
    let mut types: Vec<_> = chosen.into_iter().collect();
    types.sort(); // deterministic case order
    let cases: Vec<TypeSwitchCase> = types
        .into_iter()
        .map(|(ty, method)| TypeSwitchCase {
            types: vec![ty],
            body: vec![Stmt::Return(
                vec![Expr::Call {
                    func: Box::new(Expr::Selector {
                        recv: Box::new(Expr::Ident("$t".to_string())),
                        field: method.to_string(),
                    }),
                    args: vec![],
                    spread: false,
                    line: 0,
                }],
                0,
            )],
        })
        .collect();
    prog.funcs.push(Func {
        name: "$stringify".to_string(),
        receiver: None,
        params: vec![Param {
            name: "$v".to_string(),
            ty: "any".to_string(),
        }],
        variadic: false,
        results: vec!["any".to_string()],
        result_names: vec![String::new()],
        body: vec![
            Stmt::TypeSwitch {
                init: None,
                bind: Some("$t".to_string()),
                expr: Expr::Ident("$v".to_string()),
                cases,
                default: None,
                line: 0,
            },
            Stmt::Return(vec![Expr::Ident("$v".to_string())], 0),
        ],
        line: 0,
    });
}

/// Load `path` and its (source) imports depth-first, qualifying each package's
/// names, recording load order (dependencies before dependents).
fn load_recursive(
    path: &str,
    loaded: &mut HashMap<String, Program>,
    order: &mut Vec<String>,
) -> Result<(), String> {
    if NATIVE.contains(&path) || loaded.contains_key(path) {
        return Ok(());
    }
    let src = resolve_source(path)
        .ok_or_else(|| format!("go-rs: cannot find package `{path}` (not vendored)"))?;
    let mut prog = crate::parse(&src)?;
    // Load this package's own imports first.
    for dep in prog.imports.clone() {
        load_recursive(&dep, loaded, order)?;
    }
    qualify(&mut prog, path, true);
    loaded.insert(path.to_string(), prog);
    order.push(path.to_string());
    Ok(())
}

/// Rewrite a package's top-level names — funcs, types, methods, and
/// package-level vars/consts — and every reference to them, to their qualified
/// `path.Name` form, and rewrite `alias.X` selectors on imported source packages
/// to `importpath.X`.
fn qualify(prog: &mut Program, path: &str, rename: bool) {
    // The package's own top-level names — only qualified when `rename` (an
    // imported package); `main` keeps its own names, rewriting only references
    // into imported source packages.
    let mut own: HashSet<String> = HashSet::new();
    if rename {
        for f in &prog.funcs {
            if f.receiver.is_none() {
                own.insert(f.name.clone());
            }
        }
        for t in &prog.types {
            own.insert(t.name.clone());
        }
        for i in &prog.interfaces {
            // An anonymous interface (`interface{ Unwrap() error }`) is named by
            // its method set, not by the package — every package that writes the
            // same one means the same type, so it is never qualified.
            if !is_anon_iface(&i.name) {
                own.insert(i.name.clone());
            }
        }
        for s in &prog.main {
            if let Stmt::Var { name, .. } = s {
                own.insert(name.clone());
            }
        }
        for (pkg, name) in INTRINSICS {
            if *pkg == path {
                own.insert((*name).to_string());
            }
        }
    }
    // alias → import path, for source packages only (native stay as selectors).
    let mut aliases: HashMap<String, String> = HashMap::new();
    for p in &prog.imports {
        if !NATIVE.contains(&p.as_str()) {
            aliases.insert(import_alias(p).to_string(), p.clone());
        }
    }

    let q = Qualifier {
        path: path.to_string(),
        own,
        aliases,
    };

    // Qualify declarations (only for imported packages) and rewrite references
    // (always) — types, then funcs, then package-init statements.
    for t in &mut prog.types {
        if rename {
            t.name = q.qual(&t.name);
        }
        for f in &mut t.fields {
            f.ty = q.qual_type(&f.ty);
        }
    }
    for i in &mut prog.interfaces {
        if rename && !is_anon_iface(&i.name) {
            i.name = q.qual(&i.name);
        }
    }
    for f in &mut prog.funcs {
        if rename && f.receiver.is_none() {
            f.name = q.qual(&f.name);
        }
        if let Some(r) = &mut f.receiver {
            r.ty = q.qual_type(&r.ty);
        }
        for p in &mut f.params {
            p.ty = q.qual_type(&p.ty);
        }
        for r in &mut f.results {
            *r = q.qual_type(r);
        }
        q.stmts(&mut f.body, &mut HashSet::new());
    }
    for s in &mut prog.main {
        q.stmt(s, &mut HashSet::new());
        if rename {
            if let Stmt::Var { name, .. } = s {
                *name = q.qual(name);
            }
        }
    }
}

/// Name-qualification walker for one package.
struct Qualifier {
    path: String,
    own: HashSet<String>,
    aliases: HashMap<String, String>,
}

impl Qualifier {
    fn qual(&self, name: &str) -> String {
        format!("{}.{}", self.path, name)
    }

    /// Qualify a type string: `*T`/`[]T`/`map[K]V` keep their shape; a bare own
    /// type or an `alias.T` reference is rewritten.
    fn qual_type(&self, ty: &str) -> String {
        if let Some(rest) = ty.strip_prefix('*') {
            return format!("*{}", self.qual_type(rest));
        }
        if let Some(rest) = ty.strip_prefix("[]") {
            return format!("[]{}", self.qual_type(rest));
        }
        // `[N]T` — the length is part of the type and is kept verbatim; only the
        // element name is a candidate for qualification.
        if let (Some(rest), Some(n)) = (crate::ast::array_elem_ty(ty), crate::ast::array_len_of(ty))
        {
            return format!("[{n}]{}", self.qual_type(rest));
        }
        // `alias.T` — a type from an imported source package.
        if let Some((a, t)) = ty.split_once('.') {
            if let Some(p) = self.aliases.get(a) {
                return format!("{p}.{t}");
            }
        }
        if self.own.contains(ty) {
            return self.qual(ty);
        }
        ty.to_string()
    }

    fn stmts(&self, body: &mut [Stmt], bound: &mut HashSet<String>) {
        for s in body {
            self.stmt(s, bound);
        }
    }

    fn stmt(&self, s: &mut Stmt, bound: &mut HashSet<String>) {
        match s {
            Stmt::Var { ty, init, name, .. } => {
                if let Some(t) = ty {
                    *t = self.qual_type(t);
                }
                if let Some(e) = init {
                    self.expr(e, bound);
                }
                bound.insert(name.clone());
            }
            Stmt::Short { names, values, .. } => {
                for v in values {
                    self.expr(v, bound);
                }
                for n in names {
                    bound.insert(n.clone());
                }
            }
            Stmt::Assign { target, value, .. } => {
                self.expr(target, bound);
                self.expr(value, bound);
            }
            Stmt::AssignMulti {
                targets, values, ..
            } => {
                targets.iter_mut().for_each(|e| self.expr(e, bound));
                values.iter_mut().for_each(|e| self.expr(e, bound));
            }
            Stmt::IncDec { target, .. } => self.expr(target, bound),
            Stmt::ExprStmt(e) => self.expr(e, bound),
            Stmt::Return(vs, _) => vs.iter_mut().for_each(|e| self.expr(e, bound)),
            Stmt::If {
                init,
                cond,
                then,
                els,
                ..
            } => {
                if let Some(i) = init {
                    self.stmt(i, bound);
                }
                self.expr(cond, bound);
                self.stmts(then, &mut bound.clone());
                self.stmts(els, &mut bound.clone());
            }
            Stmt::For {
                init,
                cond,
                post,
                body,
                ..
            } => {
                let mut inner = bound.clone();
                if let Some(i) = init {
                    self.stmt(i, &mut inner);
                }
                if let Some(c) = cond {
                    self.expr(c, &inner);
                }
                if let Some(p) = post {
                    self.stmt(p, &mut inner);
                }
                self.stmts(body, &mut inner);
            }
            Stmt::ForRange {
                key,
                val,
                iter,
                body,
                ..
            } => {
                self.expr(iter, bound);
                let mut inner = bound.clone();
                inner.extend(key.iter().cloned());
                inner.extend(val.iter().cloned());
                self.stmts(body, &mut inner);
            }
            Stmt::Go { call, .. } | Stmt::Defer { call, .. } => self.expr(call, bound),
            Stmt::Send { chan, val, .. } => {
                self.expr(chan, bound);
                self.expr(val, bound);
            }
            Stmt::Select { cases, default, .. } => {
                for c in cases {
                    match &mut c.comm {
                        SelectComm::Recv {
                            chan,
                            bind,
                            ok_bind,
                        } => {
                            self.expr(chan, bound);
                            for b in [bind, ok_bind].into_iter().flatten() {
                                bound.insert(b.clone());
                            }
                        }
                        SelectComm::Send { chan, val } => {
                            self.expr(chan, bound);
                            self.expr(val, bound);
                        }
                    }
                    self.stmts(&mut c.body, &mut bound.clone());
                }
                if let Some(d) = default {
                    self.stmts(d, &mut bound.clone());
                }
            }
            Stmt::Switch {
                init,
                tag,
                cases,
                default,
                ..
            } => {
                let mut inner = bound.clone();
                if let Some(i) = init {
                    self.stmt(i, &mut inner);
                }
                if let Some(t) = tag {
                    self.expr(t, &inner);
                }
                for c in cases {
                    c.exprs.iter_mut().for_each(|e| self.expr(e, &inner));
                    self.stmts(&mut c.body, &mut inner.clone());
                }
                if let Some(d) = default {
                    self.stmts(d, &mut inner.clone());
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
                let mut inner = bound.clone();
                if let Some(i) = init {
                    self.stmt(i, &mut inner);
                }
                self.expr(expr, &inner);
                if let Some(b) = bind {
                    inner.insert(b.clone());
                }
                for c in cases {
                    for t in &mut c.types {
                        *t = self.qual_type(t);
                    }
                    self.stmts(&mut c.body, &mut inner.clone());
                }
                if let Some(d) = default {
                    self.stmts(d, &mut inner.clone());
                }
            }
            Stmt::Block(b) => self.stmts(b, &mut bound.clone()),
            Stmt::Fallthrough(_) | Stmt::Break(..) | Stmt::Continue(..) => {}
        }
    }

    fn expr(&self, e: &mut Expr, bound: &HashSet<String>) {
        match e {
            // A bare identifier referring to this package's own top-level name.
            Expr::Ident(n) => {
                if !bound.contains(n) && self.own.contains(n) {
                    *n = self.qual(n);
                }
            }
            // `alias.field` on an imported source package → a qualified identifier.
            Expr::Selector { recv, field } => {
                if let Expr::Ident(a) = recv.as_ref() {
                    if !bound.contains(a) {
                        if let Some(p) = self.aliases.get(a) {
                            *e = Expr::Ident(format!("{p}.{field}"));
                            return;
                        }
                    }
                }
                self.expr(recv, bound);
            }
            Expr::Unary { rhs, .. } => self.expr(rhs, bound),
            Expr::Binary { lhs, rhs, .. } => {
                self.expr(lhs, bound);
                self.expr(rhs, bound);
            }
            Expr::Call { func, args, .. } => {
                self.expr(func, bound);
                args.iter_mut().for_each(|a| self.expr(a, bound));
            }
            Expr::Index { recv, index } => {
                self.expr(recv, bound);
                self.expr(index, bound);
            }
            Expr::Slice {
                recv,
                low,
                high,
                max,
            } => {
                self.expr(recv, bound);
                for e in [low, high, max].into_iter().flatten() {
                    self.expr(e, bound);
                }
            }
            Expr::TypeAssert { expr, ty } => {
                self.expr(expr, bound);
                if ty != "type" {
                    *ty = self.qual_type(ty);
                }
            }
            Expr::SliceLit { elem_ty, elems, .. } => {
                *elem_ty = self.qual_type(elem_ty);
                elems.iter_mut().for_each(|e| self.expr(e, bound));
            }
            Expr::MapLit {
                key_ty,
                val_ty,
                pairs,
            } => {
                *key_ty = self.qual_type(key_ty);
                *val_ty = self.qual_type(val_ty);
                for (k, v) in pairs {
                    self.expr(k, bound);
                    self.expr(v, bound);
                }
            }
            Expr::StructLit { type_name, fields } => {
                *type_name = self.qual_type(type_name);
                for (_, v) in fields {
                    self.expr(v, bound);
                }
            }
            Expr::Make { len, elem_zero, .. } => {
                if let Some(l) = len {
                    self.expr(l, bound);
                }
                self.expr(elem_zero, bound);
            }
            Expr::MakeChan { cap, .. } => {
                if let Some(c) = cap {
                    self.expr(c, bound);
                }
            }
            Expr::Recv { chan } => self.expr(chan, bound),
            Expr::FuncLit { params, body, .. } => {
                let mut inner = bound.clone();
                for p in params {
                    p.ty = self.qual_type(&p.ty);
                    inner.insert(p.name.clone());
                }
                self.stmts(body, &mut inner);
            }
            Expr::Int(_) | Expr::Float(..) | Expr::Str(_) | Expr::Bool(_) => {}
        }
    }
}

/// Locate a package's source. Checks the embedded vendored standard library
/// first, then `$GOROOT/src/<path>` for development.
fn resolve_source(path: &str) -> Option<String> {
    // 1. The installed stdlib under `~/.go-rs/src/<path>` (see `go install-std`).
    if let Some(home) = gors_home() {
        let dir = home.join("src").join(path);
        if let Some(src) = read_package_dir(&dir) {
            return Some(src);
        }
    }
    // 2. The stdlib vendored into the binary.
    if let Some(src) = vendored_source(path) {
        return Some(src);
    }
    // 3. A local Go toolchain's `$GOROOT/src/<path>` (development fallback).
    let goroot = std::env::var("GOROOT").ok().or_else(goroot_from_go)?;
    let dir = std::path::Path::new(&goroot).join("src").join(path);
    read_package_dir(&dir)
}

/// go-rs's home directory: `$GO_RS_HOME`, else `~/.go-rs`.
pub fn gors_home() -> Option<std::path::PathBuf> {
    if let Ok(h) = std::env::var("GO_RS_HOME") {
        return Some(std::path::PathBuf::from(h));
    }
    std::env::var("HOME")
        .ok()
        .map(|h| std::path::PathBuf::from(h).join(".go-rs"))
}

/// The vendored standard-library packages (import path → single-file source),
/// written to `~/.go-rs/src/<path>/<name>.go` by `go install-std`.
pub const VENDORED: &[&str] = &["errors", "sync", "unicode/utf16", "cmp"];

/// Install the vendored standard library into `~/.go-rs/src/`. Returns the
/// number of packages written.
pub fn install_stdlib() -> Result<usize, String> {
    let home = gors_home().ok_or("go-rs: cannot determine home directory")?;
    let mut n = 0;
    for &path in VENDORED {
        let src = vendored_source(path)
            .ok_or_else(|| format!("go-rs: vendored source missing for `{path}`"))?;
        let dir = home.join("src").join(path);
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("go-rs: cannot create {}: {e}", dir.display()))?;
        let leaf = path.rsplit('/').next().unwrap_or(path);
        let file = dir.join(format!("{leaf}.go"));
        std::fs::write(&file, src)
            .map_err(|e| format!("go-rs: cannot write {}: {e}", file.display()))?;
        n += 1;
    }
    Ok(n)
}

/// Concatenate the non-test, platform-neutral `.go` files of a package directory.
fn read_package_dir(dir: &std::path::Path) -> Option<String> {
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| is_buildable_go_file(p))
        .collect();
    files.sort();
    if files.is_empty() {
        return None;
    }
    // Split each file into its imports and its remaining body; the package is
    // then re-emitted as one clause + one deduplicated import block + all bodies.
    let mut imports: Vec<String> = Vec::new();
    let mut bodies = String::new();
    for f in files {
        if let Ok(text) = std::fs::read_to_string(&f) {
            let (imps, body) = split_file(&text);
            for i in imps {
                if !imports.contains(&i) {
                    imports.push(i);
                }
            }
            bodies.push_str(&body);
            bodies.push('\n');
        }
    }
    let mut out = String::from("package pkg\n");
    for i in &imports {
        out.push_str(&format!("import \"{i}\"\n"));
    }
    out.push_str(&bodies);
    Some(out)
}

/// Whether a file is a `.go` source we should compile: not a test, not
/// platform/arch-specific for a platform other than the host.
fn is_buildable_go_file(p: &std::path::Path) -> bool {
    let name = match p.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return false,
    };
    if !name.ends_with(".go") || name.ends_with("_test.go") {
        return false;
    }
    // `foo_linux.go` / `foo_amd64.go` — accept only host-matching suffixes.
    let stem = &name[..name.len() - 3];
    let host_os = std::env::consts::OS;
    let host_arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        a => a,
    };
    for seg in stem.split('_').skip(1) {
        if is_known_os(seg) && seg != host_os && !(seg == "darwin" && host_os == "macos") {
            return false;
        }
        if is_known_arch(seg) && seg != host_arch {
            return false;
        }
    }
    // Exclude generator/example files carrying an `ignore` build constraint
    // (`//go:build ignore` or the legacy `// +build ignore`) — they are not part
    // of the package build (e.g. math/bits/make_examples.go).
    if let Ok(text) = std::fs::read_to_string(p) {
        for line in text.lines().take(30) {
            let l = line.trim();
            if !l.is_empty() && !l.starts_with("//") && !l.starts_with("package ") {
                break; // past the header; build constraints only appear above it
            }
            if (l.starts_with("//go:build") || l.starts_with("// +build")) && l.contains("ignore") {
                return false;
            }
        }
    }
    true
}

fn is_known_os(s: &str) -> bool {
    matches!(
        s,
        "linux"
            | "darwin"
            | "windows"
            | "freebsd"
            | "netbsd"
            | "openbsd"
            | "js"
            | "wasip1"
            | "plan9"
    )
}

fn is_known_arch(s: &str) -> bool {
    matches!(
        s,
        "amd64"
            | "arm64"
            | "386"
            | "arm"
            | "wasm"
            | "riscv64"
            | "ppc64"
            | "ppc64le"
            | "s390x"
            | "mips64"
    )
}

/// Split a Go source file into its import paths and the body without the
/// `package` clause or `import` declarations (line-based; relies on the
/// gofmt'd one-declaration-per-line layout of stdlib source).
fn split_file(src: &str) -> (Vec<String>, String) {
    let mut imports = Vec::new();
    let mut body = String::new();
    let mut lines = src.lines().peekable();
    while let Some(line) = lines.next() {
        let t = line.trim();
        if t.starts_with("package ") {
            continue;
        }
        if t == "import (" || t.starts_with("import (") {
            // Grouped import block until the closing `)`.
            for l in lines.by_ref() {
                let lt = l.trim();
                if lt == ")" {
                    break;
                }
                if let Some(p) = import_path_in(lt) {
                    imports.push(p);
                }
            }
            continue;
        }
        if let Some(rest) = t.strip_prefix("import ") {
            if let Some(p) = import_path_in(rest) {
                imports.push(p);
            }
            continue;
        }
        body.push_str(line);
        body.push('\n');
    }
    (imports, body)
}

/// Extract the quoted path from an import spec line (`_ "x"`, `alias "x"`, `"x"`).
fn import_path_in(line: &str) -> Option<String> {
    let start = line.find('"')?;
    let end = line[start + 1..].find('"')? + start + 1;
    Some(line[start + 1..end].to_string())
}

fn goroot_from_go() -> Option<String> {
    let out = std::process::Command::new("go")
        .args(["env", "GOROOT"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Vendored standard-library source embedded in the binary (populated as
/// packages are verified to run on go-rs).
fn vendored_source(path: &str) -> Option<String> {
    crate::stdlib_vendor::source(path)
}
