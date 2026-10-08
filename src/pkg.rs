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
const INTRINSICS: &[(&str, &str)] = &[
    ("errors", "runtimeTypeTag"),
    ("os", "writeFd"),
    ("slices", "sliceOverlap"),
];

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
        main.defined.extend(pkg.defined);
    }
    init_globals.extend(std::mem::take(&mut main.main));
    main.main = init_globals;
    // `strconv`'s Go half reaches `ErrSyntax`, so it is linked before the
    // check below that decides whether the error types are needed.
    if uses_strconv_source(&main) {
        add_strconv_source(&mut main)?;
    }
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
    if uses_recover(&main) {
        add_runtime_error_types(&mut main);
    }
    // `strings.Builder` is a *type* in a package whose functions are host
    // builtins, so it cannot arrive through the source-package path; it is
    // synthesized and qualified instead. Gated on the import, because a program
    // that never names `strings` cannot name the type either.
    if main.imports.iter().any(|p| p == "strings") {
        add_strings_builder(&mut main)?;
        add_strings_func(&mut main)?;
        add_strings_source(&mut main)?;
    }
    // `os.Stdout` / `os.Stderr` the same way. Their initializers are
    // package-level vars, so they are spliced ahead of the program's own
    // globals — a `var out = os.Stdout` must see them already built.
    if main.imports.iter().any(|p| p == "os") {
        add_os_file(&mut main)?;
    }
    if main.imports.iter().any(|p| p == "sort") {
        add_sort_source(&mut main)?;
    }
    if main.imports.iter().any(|p| p == "math") {
        add_math_source(&mut main)?;
    }
    add_sort_search(&mut main);
    add_stringify(&mut main);
    Ok(main)
}

/// The names package `sort` declares in Go source (`goroot/sort.go`) rather
/// than as host builtins — each calls `Less` / `Swap` methods or a `less`
/// closure, which a host builtin cannot, or is a type. They are synthesized
/// and qualified under `sort`, and a program's `sort.<Name>` is rewritten to
/// the qualified identifier (`Qualifier::source_half`).
pub const SORT_SOURCE: &[&str] = &[
    "Sort",
    "Stable",
    "IsSorted",
    "Reverse",
    "Slice",
    "SliceStable",
    "SliceIsSorted",
    "Interface",
    "IntSlice",
    "Float64Slice",
    "StringSlice",
];

/// Synthesize package `sort`'s Go half — `Interface`, the `IntSlice` family,
/// and Go's own pdqsort / SymMerge (`goroot/sort.go`). It has to be Go's
/// algorithm: the order an unstable sort leaves equal elements in is output.
fn add_sort_source(prog: &mut Program) -> Result<(), String> {
    let mut pkg = crate::parse(include_str!("../goroot/sort.go"))?;
    qualify(&mut pkg, "sort", true);
    prog.types.append(&mut pkg.types);
    prog.interfaces.append(&mut pkg.interfaces);
    prog.defined.append(&mut pkg.defined);
    prog.funcs.append(&mut pkg.funcs);
    let inits = std::mem::take(&mut pkg.main);
    prog.main.splice(0..0, inits);
    Ok(())
}

/// Synthesize the rest of package `sort`: the binary searches and the
/// "is it already sorted" predicates.
///
/// Written in Go and parsed for the same reason `goroot/sort.go` is: `sort.Search`
/// takes a VM closure, which a host builtin cannot call.
/// The four that take no closure ride along in the same form rather than
/// becoming host builtins, so the whole package is described in one place and
/// the search halves cannot drift apart.
///
/// `Search` is Go's contract exactly: the smallest `i` in `[0, n)` for which
/// `f(i)` is true, and `n` when there is none — which is what makes
/// `SearchInts` answer `len(a)` for a value past the end rather than -1.
fn add_sort_search(prog: &mut Program) {
    let each: [(&str, &str); 7] = [
        (
            "$sortSearch",
            "func h(n int, f func(int) bool) int {\n\
             \ti, j := 0, n\n\
             \tfor i < j {\n\
             \t\tm := (i + j) / 2\n\
             \t\tif !f(m) {\n\t\t\ti = m + 1\n\t\t} else {\n\t\t\tj = m\n\t\t}\n\
             \t}\n\treturn i\n}\n",
        ),
        (
            "$searchInts",
            "func h(a []int, x int) int {\n\
             \ti, j := 0, len(a)\n\
             \tfor i < j {\n\t\tm := (i + j) / 2\n\
             \t\tif a[m] < x {\n\t\t\ti = m + 1\n\t\t} else {\n\t\t\tj = m\n\t\t}\n\
             \t}\n\treturn i\n}\n",
        ),
        (
            "$searchStrings",
            "func h(a []string, x string) int {\n\
             \ti, j := 0, len(a)\n\
             \tfor i < j {\n\t\tm := (i + j) / 2\n\
             \t\tif a[m] < x {\n\t\t\ti = m + 1\n\t\t} else {\n\t\t\tj = m\n\t\t}\n\
             \t}\n\treturn i\n}\n",
        ),
        (
            "$searchFloat64s",
            "func h(a []float64, x float64) int {\n\
             \ti, j := 0, len(a)\n\
             \tfor i < j {\n\t\tm := (i + j) / 2\n\
             \t\tif a[m] < x {\n\t\t\ti = m + 1\n\t\t} else {\n\t\t\tj = m\n\t\t}\n\
             \t}\n\treturn i\n}\n",
        ),
        (
            "$intsAreSorted",
            "func h(a []int) bool {\n\tfor i := 1; i < len(a); i++ {\n\
             \t\tif a[i] < a[i-1] {\n\t\t\treturn false\n\t\t}\n\t}\n\treturn true\n}\n",
        ),
        (
            "$stringsAreSorted",
            "func h(a []string) bool {\n\tfor i := 1; i < len(a); i++ {\n\
             \t\tif a[i] < a[i-1] {\n\t\t\treturn false\n\t\t}\n\t}\n\treturn true\n}\n",
        ),
        (
            "$float64sAreSorted",
            "func h(a []float64) bool {\n\tfor i := 1; i < len(a); i++ {\n\
             \t\tif a[i] < a[i-1] {\n\t\t\treturn false\n\t\t}\n\t}\n\treturn true\n}\n",
        ),
    ];
    for (name, body) in each {
        let src = format!("package p\n{body}");
        if let Ok(mut p) = crate::parse(&src) {
            if let Some(mut f) = p.funcs.pop() {
                f.name = name.to_string();
                prog.funcs.push(f);
            }
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
    // The selector itself, called or used as a value (`conv := strconv.Atoi`):
    // the walk reaches a call's callee too.
    program_has_expr(prog, &|e| {
        ERROR_MAKERS.iter().any(|(p, f)| is_selector(e, p, f))
    })
}

/// Whether the program can observe a `*strconv.NumError` — it either runs a
/// conversion that builds one, or names one of the sentinels such a value wraps.
fn uses_strconv_error(prog: &Program) -> bool {
    /// The `strconv` conversions whose second result is a `*NumError`.
    const PARSERS: &[&str] = &["Atoi", "ParseInt", "ParseUint", "ParseFloat", "ParseBool"];
    // Called or used as a value; the walk reaches a call's callee too.
    program_has_expr(prog, &|e| {
        PARSERS.iter().any(|f| is_selector(e, "strconv", f))
            || is_selector(e, "strconv", "ErrSyntax")
            || is_selector(e, "strconv", "ErrRange")
    })
}

/// Synthesize `os.Stdout` / `os.Stderr` and the `*os.File` behind them. Their
/// package-level initializers are spliced to the *front* of the program's
/// globals, because a user global may read one.
fn add_os_file(prog: &mut Program) -> Result<(), String> {
    let mut pkg = crate::parse(include_str!("../goroot/os_file.go"))?;
    qualify(&mut pkg, "os", true);
    prog.types.append(&mut pkg.types);
    prog.funcs.append(&mut pkg.funcs);
    let inits = std::mem::take(&mut pkg.main);
    prog.main.splice(0..0, inits);
    Ok(())
}

/// The `strings` functions taking a function argument, synthesized from
/// `goroot/strings_func.go` as `$strings<Name>` — a host builtin cannot call the
/// VM closure they are handed. The compiler routes `strings.<Name>` to them.
pub const STRINGS_FUNC: &[&str] = &[
    "Map",
    "IndexFunc",
    "LastIndexFunc",
    "ContainsFunc",
    "TrimLeftFunc",
    "TrimRightFunc",
    "TrimFunc",
    "FieldsFunc",
];

fn add_strings_func(prog: &mut Program) -> Result<(), String> {
    let pkg = crate::parse(include_str!("../goroot/strings_func.go"))?;
    for mut f in pkg.funcs {
        if STRINGS_FUNC.contains(&f.name.as_str()) {
            f.name = format!("$strings{}", f.name);
            prog.funcs.push(f);
        }
    }
    Ok(())
}

/// The names package `strings` declares in Go source (`goroot/strings_source.go`)
/// rather than as host builtins: the functions with more than one result, the
/// ones Go builds on its own helpers, and the `Replacer` type. Rewritten and
/// qualified the way [`SORT_SOURCE`] is.
pub const STRINGS_SOURCE: &[&str] = &[
    "Cut",
    "CutPrefix",
    "CutSuffix",
    "SplitAfter",
    "SplitAfterN",
    "LastIndexAny",
    "Replacer",
    "NewReplacer",
];

/// The names package `math` declares in Go source (`goroot/math_source.go`)
/// rather than as host builtins — Go's own portable bodies, which also decide
/// the last bit of each result. Rewritten and qualified the way
/// [`SORT_SOURCE`] is.
pub const MATH_SOURCE: &[&str] = &[
    "Modf",
    "Frexp",
    "Ldexp",
    "Dim",
    "Remainder",
    "Log1p",
    "Expm1",
    "Gamma",
    "Erf",
    "Erfc",
    "Nextafter",
    "Nextafter32",
    "RoundToEven",
];

/// Synthesize package `math`'s Go half ([`MATH_SOURCE`]) and qualify it under
/// `math`. Its tables are package-level vars, spliced ahead of the program's
/// own globals.
fn add_math_source(prog: &mut Program) -> Result<(), String> {
    let mut pkg = crate::parse(include_str!("../goroot/math_source.go"))?;
    qualify(&mut pkg, "math", true);
    prog.funcs.append(&mut pkg.funcs);
    let inits = std::mem::take(&mut pkg.main);
    prog.main.splice(0..0, inits);
    Ok(())
}

/// The names package `strconv` declares in Go source
/// (`goroot/strconv_source.go`) rather than as host builtins: the unquoting
/// functions, which return several results, and the `Append*` family.
/// Rewritten and qualified the way [`SORT_SOURCE`] is.
pub const STRCONV_SOURCE: &[&str] = &[
    "Unquote",
    "UnquoteChar",
    "QuotedPrefix",
    "AppendBool",
    "AppendInt",
    "AppendUint",
    "AppendFloat",
    "AppendQuote",
    "AppendQuoteToASCII",
    "AppendQuoteToGraphic",
    "AppendQuoteRune",
    "AppendQuoteRuneToASCII",
    "AppendQuoteRuneToGraphic",
];

/// Whether the program names one of [`STRCONV_SOURCE`] — called or used as a
/// value. Only then is the Go half linked. `main` is already qualified here, so
/// the reference is the identifier `strconv.<Name>` rather than a selector.
fn uses_strconv_source(prog: &Program) -> bool {
    program_has_expr(prog, &|e| {
        matches!(e, Expr::Ident(n)
            if n.strip_prefix("strconv.").is_some_and(|f| STRCONV_SOURCE.contains(&f)))
    })
}

/// Synthesize package `strconv`'s Go half ([`STRCONV_SOURCE`]) and qualify
/// it under `strconv`.
fn add_strconv_source(prog: &mut Program) -> Result<(), String> {
    let mut pkg = crate::parse(include_str!("../goroot/strconv_source.go"))?;
    qualify(&mut pkg, "strconv", true);
    prog.funcs.append(&mut pkg.funcs);
    Ok(())
}

/// Synthesize package `strings`' Go half ([`STRINGS_SOURCE`]) and qualify it
/// under `strings`.
fn add_strings_source(prog: &mut Program) -> Result<(), String> {
    let mut pkg = crate::parse(include_str!("../goroot/strings_source.go"))?;
    qualify(&mut pkg, "strings", true);
    prog.types.append(&mut pkg.types);
    prog.funcs.append(&mut pkg.funcs);
    Ok(())
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
            embedded: Vec::new(),
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

/// Whether the program calls `recover` — the one way it can hold the value a
/// run-time fault panics with.
fn uses_recover(prog: &Program) -> bool {
    program_has_expr(
        prog,
        &|e| matches!(e, Expr::Call { func, .. } if matches!(func.as_ref(), Expr::Ident(n) if n == "recover")),
    )
}

/// Synthesize the `runtime` error types a run-time fault panics with, so the
/// value `recover()` returns is an `error` (`r.(error)` holds) whose `%v` is
/// its message and whose `%T` is Go's: `runtime.boundsError` for an index or
/// slice bound, `runtime.errorString` for a nil dereference or a division by
/// zero, `runtime.plainError` for a write to a nil map, and
/// `*runtime.TypeAssertionError` for a failed assertion (runtime/error.go).
///
/// Go's `errorString` and `plainError` are `string` types and `boundsError`
/// keeps its operands to format later; each is a one-field struct holding the
/// finished message here, which only `%#v` could tell apart. The host builds
/// the values (`host::runtime_error_value`).
fn add_runtime_error_types(prog: &mut Program) {
    let src = "package runtime\n\
        type boundsError struct{ s string }\n\
        func (e boundsError) Error() string { return e.s }\n\
        func (e boundsError) RuntimeError() {}\n\
        type errorString struct{ s string }\n\
        func (e errorString) Error() string { return e.s }\n\
        func (e errorString) RuntimeError() {}\n\
        type plainError struct{ s string }\n\
        func (e plainError) Error() string { return e.s }\n\
        func (e plainError) RuntimeError() {}\n\
        type TypeAssertionError struct{ s string }\n\
        func (e *TypeAssertionError) Error() string { return e.s }\n\
        func (*TypeAssertionError) RuntimeError() {}\n";
    let Ok(mut p) = crate::parse(src) else { return };
    qualify(&mut p, "runtime", true);
    prog.types.append(&mut p.types);
    prog.funcs.append(&mut p.funcs);
}

/// Synthesize the `$stringify` helper — a type switch over every type with a
/// zero-argument `Error()`/`String()` method that calls it — so `fmt` prints
/// error and `Stringer` values via their method (Go's fmt interface handling).
/// The compiler wraps each `fmt.Print*`/`Sprint*` argument with a call to it.
///
/// Go's `fmt` applies the same rule to the exported fields of a struct it
/// prints (`printValue` calls `handleMethods` on any field it can take an
/// interface of), so a struct with such a field gets a case too: it returns a
/// display copy whose method-bearing fields are already rendered
/// (`$withFields`). An unexported field is printed as its plain value, as Go
/// cannot call a method through it.
fn add_stringify(prog: &mut Program) {
    // Every type that has `String()` / `Error()` — declared, or promoted from
    // an embedded field — with whether a *value* of it has the method (Go's
    // method-set rule: a pointer-receiver method belongs to `*T` only).
    let mut candidates: Vec<String> = prog.types.iter().map(|t| t.name.clone()).collect();
    candidates.extend(
        prog.funcs
            .iter()
            .filter_map(|f| f.receiver.as_ref())
            .map(|r| r.ty.trim_start_matches('*').to_string()),
    );
    candidates.sort();
    candidates.dedup();
    // type → (the method a value calls, the method a pointer calls); `Error`
    // is preferred over `String`, as `fmt` checks `error` first.
    let mut reach: Vec<(String, Option<&str>, &str)> = Vec::new();
    for ty in candidates {
        let error = method_reach(prog, &ty, "Error", 0);
        let string = method_reach(prog, &ty, "String", 0);
        let on_value = match (error, string) {
            (Some(true), _) => Some("Error"),
            (_, Some(true)) => Some("String"),
            _ => None,
        };
        let on_ptr = match (error, string) {
            (Some(_), _) => "Error",
            (_, Some(_)) => "String",
            _ => continue,
        };
        reach.push((ty, on_value, on_ptr));
    }
    if reach.is_empty() {
        return;
    }
    let shown = ShownTypes::new(prog, &reach);

    // `fmt` prints a slice or array element through its method too — `[]Color`
    // prints `[G B]` — and an element is not an operand the per-argument
    // helper sees. For each program type (a package's own are left alone) a
    // `$stringifyAll_T(xs []T) []any` sends the elements through `$stringify`,
    // and the compiler routes a `[]T` / `[N]T` operand through it; `[]any`
    // gets one too. Go source cannot name `$stringify`, so the call is written
    // against a placeholder and renamed in the parsed loop body.
    let mut all_types: Vec<String> = shown.any_method.iter().cloned().collect();
    all_types.extend(shown.rebuilt.iter().cloned());
    all_types.retain(|t| !t.contains('.'));
    all_types.sort();
    all_types.dedup();
    for (elem, name) in std::iter::once(("any".to_string(), "any".to_string()))
        .chain(all_types.iter().map(|t| (t.clone(), t.clone())))
    {
        let src = format!(
            "package p\nfunc h(xs []{elem}) []any {{\n\tout := make([]any, len(xs))\n\tfor i := 0; i < len(xs); i++ {{\n\t\tout[i] = placeholder(xs[i])\n\t}}\n\treturn out\n}}\n"
        );
        let Ok(mut p) = crate::parse(&src) else {
            continue;
        };
        let Some(mut f) = p.funcs.pop() else { continue };
        for s in &mut f.body {
            if let Stmt::For { body, .. } = s {
                for s in body {
                    if let Stmt::Assign {
                        value: Expr::Call { func, .. },
                        ..
                    } = s
                    {
                        **func = Expr::Ident("$stringify".to_string());
                    }
                }
            }
        }
        f.name = format!("$stringifyAll_{name}");
        prog.funcs.push(f);
    }

    // A struct value and a pointer to one are distinct dynamic types: the value
    // has only its value-receiver methods, the pointer has them all. So a
    // struct's `T` case calls the method its value has, if any, and its `*T`
    // case the one the pointer has. Any other type has one tag for both.
    let call = |func: Expr, args: Vec<Expr>| Expr::Call {
        func: Box::new(func),
        args,
        spread: false,
        line: 0,
    };
    let method_call = |method: &str| {
        call(
            Expr::Selector {
                recv: Box::new(Expr::Ident("$t".to_string())),
                field: method.to_string(),
            },
            vec![],
        )
    };
    let mut arms: Vec<(String, Expr)> = Vec::new();
    for (ty, on_value, on_ptr) in &reach {
        if !shown.structs.contains(ty.as_str()) {
            arms.push((ty.clone(), method_call(on_ptr)));
            continue;
        }
        if let Some(m) = on_value {
            arms.push((ty.clone(), method_call(m)));
        }
        arms.push((format!("*{ty}"), method_call(on_ptr)));
    }
    // A struct that prints some field through a method: `$withFields($t,
    // "F", rendered, …)` copies it with those fields replaced. The pointer
    // gets the same case unless it has a method of its own.
    let mut rebuilt: Vec<&String> = shown.rebuilt.iter().collect();
    rebuilt.sort();
    for ty in rebuilt {
        let Some(decl) = prog.types.iter().find(|t| &t.name == ty) else {
            continue;
        };
        let mut args = vec![Expr::Ident("$t".to_string())];
        for f in &decl.fields {
            let Some(each) = shown.field_rendering(&f.name, &f.ty) else {
                continue;
            };
            let field = Expr::Selector {
                recv: Box::new(Expr::Ident("$t".to_string())),
                field: f.name.clone(),
            };
            let helper = match each {
                Some(elem) => format!("$stringifyAll_{elem}"),
                None => "$stringify".to_string(),
            };
            args.push(Expr::Str(f.name.clone()));
            args.push(call(Expr::Ident(helper), vec![field]));
        }
        let rebuild = call(Expr::Ident("$withFields".to_string()), args);
        arms.push((ty.clone(), rebuild.clone()));
        if !shown.any_method.contains(ty) {
            arms.push((format!("*{ty}"), rebuild));
        }
    }
    let cases: Vec<TypeSwitchCase> = arms
        .into_iter()
        .map(|(ty, value)| TypeSwitchCase {
            types: vec![ty],
            body: vec![Stmt::Return(vec![value], 0)],
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

/// Which types `fmt` prints through a method, as `add_stringify` sees them.
struct ShownTypes {
    /// Every struct type name.
    structs: HashSet<String>,
    /// Types a *value* of which has `String()` / `Error()`.
    on_value: HashSet<String>,
    /// Types whose value or pointer has one.
    any_method: HashSet<String>,
    /// Interface types, whose dynamic value decides at run time.
    interfaces: HashSet<String>,
    /// Structs without a method of their own on the value that print some
    /// exported field through one — directly, or through a nested struct.
    rebuilt: HashSet<String>,
}

impl ShownTypes {
    fn new(prog: &Program, reach: &[(String, Option<&str>, &str)]) -> ShownTypes {
        let mut interfaces: HashSet<String> =
            prog.interfaces.iter().map(|i| i.name.clone()).collect();
        interfaces.extend(["any", "error", "fmt.Stringer"].map(str::to_string));
        let mut shown = ShownTypes {
            structs: prog.types.iter().map(|t| t.name.clone()).collect(),
            on_value: reach
                .iter()
                .filter(|(_, v, _)| v.is_some())
                .map(|(t, _, _)| t.clone())
                .collect(),
            any_method: reach.iter().map(|(t, _, _)| t.clone()).collect(),
            interfaces,
            rebuilt: HashSet::new(),
        };
        loop {
            let grew: Vec<String> = prog
                .types
                .iter()
                .filter(|t| !shown.on_value.contains(&t.name) && !shown.rebuilt.contains(&t.name))
                .filter(|t| {
                    t.fields
                        .iter()
                        .any(|f| shown.field_rendering(&f.name, &f.ty).is_some())
                })
                .map(|t| t.name.clone())
                .collect();
            if grew.is_empty() {
                return shown;
            }
            shown.rebuilt.extend(grew);
        }
    }

    /// How `fmt` renders field `name` of written type `fty` when that differs
    /// from the plain value: `Some(None)` through `$stringify`,
    /// `Some(Some(elem))` element-wise through `$stringifyAll_elem`, `None`
    /// plainly — an unexported field, or one whose type has no method.
    fn field_rendering(&self, name: &str, fty: &str) -> Option<Option<String>> {
        if !name.starts_with(|c: char| c.is_uppercase()) {
            return None;
        }
        let shows = |t: &str| match t.strip_prefix('*') {
            Some(inner) => self.any_method.contains(inner),
            None => {
                self.on_value.contains(t)
                    || self.rebuilt.contains(t)
                    || self.interfaces.contains(t)
                    || t.starts_with("interface{")
            }
        };
        // `[]E` and `[N]E` both split at the first `]`.
        match fty.strip_prefix('[').and_then(|r| r.split_once(']')) {
            Some((_, elem)) => {
                let base = elem.trim_start_matches('*');
                // Interface elements carry their dynamic types, so the one
                // `[]any` helper serves them; a package's own types have none.
                let helper = match self.interfaces.contains(base) {
                    true => "any",
                    false => base,
                };
                (shows(elem) && !helper.contains('.')).then(|| Some(helper.to_string()))
            }
            None => shows(fty).then_some(None),
        }
    }
}

/// Whether type `ty` has a zero-argument `method` — `Some(on_value)` with
/// whether a value of `ty` has it, `None` when neither `ty` nor `*ty` does. A
/// declared method is on the value unless its receiver is a pointer; otherwise
/// the first embedded field that supplies it promotes it — an embedded `*T`
/// onto the value too, an embedded `T` only as `T`'s value has it. (Two fields
/// supplying it at one depth is ambiguous in Go, which the first-found rule
/// does not model; see BUGS.md.)
fn method_reach(prog: &Program, ty: &str, method: &str, depth: usize) -> Option<bool> {
    let declared = prog.funcs.iter().find_map(|f| {
        let r = f.receiver.as_ref()?;
        (f.name == method && f.params.is_empty() && r.ty.trim_start_matches('*') == ty)
            .then(|| !r.ty.starts_with('*'))
    });
    if declared.is_some() || depth > 8 {
        return declared;
    }
    let t = prog.types.iter().find(|t| t.name == ty)?;
    t.fields.iter().find_map(|f| {
        if !t.is_embedded(&f.name) {
            return None; // a named field, not an embedded one
        }
        let inner = f.ty.trim_start_matches('*');
        let on_value = method_reach(prog, inner, method, depth + 1)?;
        Some(f.ty.starts_with('*') || on_value)
    })
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
        for (name, _) in &prog.defined {
            own.insert(name.clone());
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
    // alias → the names a native package declares in Go source instead
    // (`sort.Sort`, `sort.IntSlice`): a reference to one of those is rewritten
    // like a source package's, and every other name stays a host selector.
    let mut source_half: HashMap<String, &'static [&'static str]> = HashMap::new();
    for p in &prog.imports {
        if p == "sort" {
            source_half.insert(import_alias(p).to_string(), SORT_SOURCE);
        }
        if p == "strings" {
            source_half.insert(import_alias(p).to_string(), STRINGS_SOURCE);
        }
        if p == "math" {
            source_half.insert(import_alias(p).to_string(), MATH_SOURCE);
        }
        if p == "strconv" {
            source_half.insert(import_alias(p).to_string(), STRCONV_SOURCE);
        }
    }

    let q = Qualifier {
        path: path.to_string(),
        own,
        aliases,
        source_half,
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
    // `type Name <base>` over a non-struct base: named like any own type, and
    // its base may itself name one.
    for (name, base) in &mut prog.defined {
        if rename {
            *name = q.qual(name);
        }
        *base = q.qual_type(base);
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
    source_half: HashMap<String, &'static [&'static str]>,
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
            Stmt::Fallthrough(_)
            | Stmt::Break(..)
            | Stmt::Continue(..)
            | Stmt::Goto(..)
            | Stmt::Label(..) => {}
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
                        if self
                            .source_half
                            .get(a)
                            .is_some_and(|names| names.contains(&field.as_str()))
                        {
                            *e = Expr::Ident(format!("{a}.{field}"));
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

/// Locate a package's source: the standard library vendored into the
/// binary, then `~/.go-rs/src/<path>`, then `$GOROOT/src/<path>`.
///
/// The vendored copy comes first because it is the one this binary was built
/// and tested against. A copy `go install-std` wrote to `~/.go-rs/src` is a
/// snapshot of some *earlier* binary's vendored source, and letting it win
/// meant an upgrade kept running the stale package — `errors.Is` stayed
/// undefined after the binary gained it.
fn resolve_source(path: &str) -> Option<String> {
    // 1. The stdlib vendored into the binary.
    if let Some(src) = vendored_source(path) {
        return Some(src);
    }
    // 2. A package under `~/.go-rs/src/<path>`.
    if let Some(home) = gors_home() {
        let dir = home.join("src").join(path);
        if let Some(src) = read_package_dir(&dir) {
            return Some(src);
        }
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

/// Install the vendored standard library into `~/.go-rs/src/` — every package
/// in [`crate::stdlib_vendor::PACKAGES`], each as `<path>/<name>.go`. Returns
/// the number of packages written.
pub fn install_stdlib() -> Result<usize, String> {
    let home = gors_home().ok_or("go-rs: cannot determine home directory")?;
    let mut n = 0;
    for &(path, src) in crate::stdlib_vendor::PACKAGES {
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
    match std::fs::read_to_string(p) {
        Ok(text) => header_constraints_hold(&text),
        Err(_) => true,
    }
}

/// Whether the build constraints in a file's header admit it to this build.
/// A `//go:build` line decides alone when present; otherwise every legacy
/// `// +build` line must hold. `ignore` names no tag of this build, which is
/// what keeps generator files out (`math/bits/make_examples.go`), and so does
/// `race` — the pair `internal/race/{race,norace}.go` both declare `Enabled`,
/// and taking both made the package's constants disagree with themselves.
fn header_constraints_hold(text: &str) -> bool {
    let mut plus_build = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if !l.is_empty() && !l.starts_with("//") {
            break; // past the header; build constraints only appear above it
        }
        if let Some(expr) = l.strip_prefix("//go:build") {
            return build_expr_holds(expr);
        }
        if let Some(expr) = l.strip_prefix("// +build") {
            plus_build.push(expr.to_string());
        }
    }
    plus_build.iter().all(|line| {
        // `// +build a,b !c` — space-separated options OR, comma-joined terms AND.
        line.split_whitespace().any(|opt| {
            opt.split(',').all(|term| match term.strip_prefix('!') {
                Some(t) => !build_tag_holds(t),
                None => build_tag_holds(term),
            })
        })
    })
}

/// Evaluate a `//go:build` expression (`linux && (arm64 || amd64)`, `!race`)
/// the way `go/build/constraint` does: `||` binds loosest, then `&&`, then
/// `!`, with parentheses. A malformed expression holds for nothing.
fn build_expr_holds(expr: &str) -> bool {
    let mut toks = Vec::new();
    let mut rest = expr.trim();
    while !rest.is_empty() {
        let (tok, n) = if rest.starts_with("&&") || rest.starts_with("||") {
            (&rest[..2], 2)
        } else if rest.starts_with(['!', '(', ')']) {
            (&rest[..1], 1)
        } else {
            let n = rest
                .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.'))
                .unwrap_or(rest.len());
            if n == 0 {
                return false;
            }
            (&rest[..n], n)
        };
        toks.push(tok);
        rest = rest[n..].trim_start();
    }
    fn or(t: &[&str], i: &mut usize) -> Option<bool> {
        let mut v = and(t, i)?;
        while t.get(*i) == Some(&"||") {
            *i += 1;
            v |= and(t, i)?;
        }
        Some(v)
    }
    fn and(t: &[&str], i: &mut usize) -> Option<bool> {
        let mut v = not(t, i)?;
        while t.get(*i) == Some(&"&&") {
            *i += 1;
            v &= not(t, i)?;
        }
        Some(v)
    }
    fn not(t: &[&str], i: &mut usize) -> Option<bool> {
        let tok = *t.get(*i)?;
        *i += 1;
        match tok {
            "!" => not(t, i).map(|v| !v),
            "(" => {
                let v = or(t, i)?;
                if t.get(*i) != Some(&")") {
                    return None;
                }
                *i += 1;
                Some(v)
            }
            "&&" | "||" | ")" => None,
            tag => Some(build_tag_holds(tag)),
        }
    }
    let mut i = 0;
    or(&toks, &mut i)
        .filter(|_| i == toks.len())
        .unwrap_or(false)
}

/// Whether one build tag is satisfied: the host `GOOS`/`GOARCH` (with `unix`
/// for a Unix-like `GOOS`, as `go/build` defines it), the `gc` toolchain the
/// stdlib is written for, and the release tags `go1.1` … up to the language
/// level go-rs reports. `cgo`, `race` and every `goexperiment.*` are off,
/// matching `CGO_ENABLED="0"` in `go env`.
fn build_tag_holds(tag: &str) -> bool {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        o => o,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        a => a,
    };
    const UNIX: &[&str] = &[
        "aix",
        "android",
        "darwin",
        "dragonfly",
        "freebsd",
        "hurd",
        "illumos",
        "ios",
        "linux",
        "netbsd",
        "openbsd",
        "solaris",
    ];
    if let Some(minor) = tag.strip_prefix("go1.") {
        let level = crate::banner::GO_COMPAT_VERSION
            .strip_prefix("1.")
            .and_then(|m| m.parse::<u32>().ok());
        return matches!((minor.parse::<u32>(), level), (Ok(m), Some(l)) if m <= l);
    }
    tag == os || tag == arch || tag == "gc" || (tag == "unix" && UNIX.contains(&os))
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

/// The Go signature of a natively implemented stdlib function, as `go doc`
/// prints it (result names dropped), for the wrapper [`stdlib_func_value`]
/// writes when the function is used as a value rather than called. `None` for
/// a name go-rs does not implement natively.
fn stdlib_func_sig(pkg: &str, func: &str) -> Option<&'static str> {
    Some(match (pkg, func) {
        ("fmt", "Sprint" | "Sprintln") => "(a ...any) string",
        ("fmt", "Sprintf") => "(format string, a ...any) string",
        ("fmt", "Print" | "Println") => "(a ...any) (int, error)",
        ("fmt", "Printf") => "(format string, a ...any) (int, error)",
        ("strings", "ToUpper" | "ToLower" | "ToTitle" | "TrimSpace" | "Title") => {
            "(s string) string"
        }
        ("strings", "Contains" | "HasPrefix" | "HasSuffix" | "EqualFold" | "ContainsAny") => {
            "(s, t string) bool"
        }
        ("strings", "Split") => "(s, sep string) []string",
        ("strings", "Join") => "(elems []string, sep string) string",
        ("strings", "Repeat") => "(s string, count int) string",
        ("strings", "Index" | "Count" | "LastIndex" | "IndexAny" | "Compare") => {
            "(s, t string) int"
        }
        ("strings", "Replace") => "(s, old, new string, n int) string",
        ("strings", "ReplaceAll") => "(s, old, new string) string",
        ("strings", "Fields") => "(s string) []string",
        ("strings", "TrimPrefix" | "TrimSuffix" | "Trim" | "TrimLeft" | "TrimRight") => {
            "(s, t string) string"
        }
        ("strings", "ContainsRune") => "(s string, r rune) bool",
        ("strings", "IndexRune") => "(s string, r rune) int",
        ("strings", "IndexByte" | "LastIndexByte") => "(s string, c byte) int",
        ("strings", "SplitN") => "(s, sep string, n int) []string",
        ("strings", "Map") => "(mapping func(rune) rune, s string) string",
        ("strings", "IndexFunc" | "LastIndexFunc") => "(s string, f func(rune) bool) int",
        ("strings", "ContainsFunc") => "(s string, f func(rune) bool) bool",
        ("strings", "TrimFunc" | "TrimLeftFunc" | "TrimRightFunc") => {
            "(s string, f func(rune) bool) string"
        }
        ("strings", "FieldsFunc") => "(s string, f func(rune) bool) []string",
        ("sort", "Search") => "(n int, f func(int) bool) int",
        ("sort", "SearchInts") => "(a []int, x int) int",
        ("sort", "SearchStrings") => "(a []string, x string) int",
        ("sort", "SearchFloat64s") => "(a []float64, x float64) int",
        ("sort", "IntsAreSorted") => "(x []int) bool",
        ("sort", "StringsAreSorted") => "(x []string) bool",
        ("sort", "Float64sAreSorted") => "(x []float64) bool",
        ("strconv", "ParseBool") => "(str string) (bool, error)",
        ("strconv", "FormatBool") => "(b bool) string",
        ("strconv", "FormatFloat") => "(f float64, fmt byte, prec, bitSize int) string",
        ("strconv", "QuoteRune") => "(r rune) string",
        ("strconv", "Itoa") => "(i int) string",
        ("strconv", "Atoi") => "(s string) (int, error)",
        ("strconv", "ParseInt") => "(s string, base int, bitSize int) (int64, error)",
        ("strconv", "ParseUint") => "(s string, base int, bitSize int) (uint64, error)",
        ("strconv", "FormatUint") => "(i uint64, base int) string",
        ("strconv", "ParseFloat") => "(s string, bitSize int) (float64, error)",
        ("strconv", "FormatInt") => "(i int64, base int) string",
        ("strconv", "Quote" | "QuoteToASCII" | "QuoteToGraphic") => "(s string) string",
        ("strconv", "QuoteRuneToASCII" | "QuoteRuneToGraphic") => "(r rune) string",
        ("strconv", "CanBackquote") => "(s string) bool",
        ("strconv", "IsPrint" | "IsGraphic") => "(r rune) bool",
        (
            "math",
            "Abs" | "Sqrt" | "Floor" | "Ceil" | "Round" | "Trunc" | "Sin" | "Cos" | "Tan" | "Asin"
            | "Acos" | "Atan" | "Sinh" | "Cosh" | "Tanh" | "Exp" | "Log" | "Log2" | "Log10"
            | "Cbrt",
        ) => "(x float64) float64",
        ("math", "Pow" | "Mod" | "Hypot" | "Max" | "Min" | "Atan2") => "(x, y float64) float64",
        ("math", "Copysign") => "(f, sign float64) float64",
        ("math", "Float64bits") => "(f float64) uint64",
        ("math", "Float64frombits") => "(b uint64) float64",
        ("math", "Float32bits") => "(f float32) uint32",
        ("math", "Float32frombits") => "(b uint32) float32",
        ("math", "Inf") => "(sign int) float64",
        ("math", "NaN") => "() float64",
        ("math", "IsInf") => "(f float64, sign int) bool",
        ("math", "IsNaN") => "(f float64) bool",
        ("math", "Signbit") => "(x float64) bool",
        ("sort", "Ints") => "(x []int)",
        ("sort", "Strings") => "(x []string)",
        ("sort", "Float64s") => "(x []float64)",
        ("os", "Getenv") => "(key string) string",
        _ => return None,
    })
}

/// `pkg.Func` used as a value (`f := strings.ToUpper`, `apply(math.Sqrt, 2)`):
/// a function literal of `pkg.Func`'s own signature that forwards its
/// parameters to the call, so the call is lowered exactly as a direct one is.
/// `None` when `pkg.Func` is not a natively implemented function.
pub fn stdlib_func_value(pkg: &str, func: &str) -> Option<Expr> {
    let sig = stdlib_func_sig(pkg, func)?;
    let (params, results) = split_sig(sig);
    let parsed = crate::parse(&format!("package p\nfunc h{params} {results} {{}}\n")).ok()?;
    let f = parsed.funcs.into_iter().next()?;
    let args: Vec<String> = f
        .params
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if f.variadic && i + 1 == f.params.len() {
                format!("{}...", p.name)
            } else {
                p.name.clone()
            }
        })
        .collect();
    let call = format!("{pkg}.{func}({})", args.join(", "));
    let body = if results.is_empty() {
        call
    } else {
        format!("return {call}")
    };
    let src = format!("package p\nfunc h{params} {results} {{\n\t{body}\n}}\n");
    let f = crate::parse(&src).ok()?.funcs.into_iter().next()?;
    Some(Expr::FuncLit {
        params: f.params,
        results: f.results,
        body: f.body,
        variadic: f.variadic,
    })
}

/// Split a signature `(params) results` at the parameter list's closing paren.
fn split_sig(sig: &str) -> (&str, &str) {
    let close = sig.find(')').map_or(sig.len(), |i| i + 1);
    (&sig[..close], sig[close..].trim())
}

#[cfg(test)]
mod tests {
    use super::{build_expr_holds, header_constraints_hold};

    /// The tags of the host this test runs on, spelled as `go/build` spells them.
    fn host() -> (&'static str, &'static str) {
        let os = match std::env::consts::OS {
            "macos" => "darwin",
            o => o,
        };
        let arch = match std::env::consts::ARCH {
            "aarch64" => "arm64",
            "x86_64" => "amd64",
            a => a,
        };
        (os, arch)
    }

    #[test]
    fn build_expressions_follow_go_build_constraint_precedence() {
        let (os, arch) = host();
        assert!(build_expr_holds("!race"));
        assert!(!build_expr_holds("race"));
        assert!(!build_expr_holds("ignore"));
        assert!(!build_expr_holds("gccgo"));
        assert!(build_expr_holds("gc"));
        assert!(!build_expr_holds("cgo"));
        assert!(!build_expr_holds("goexperiment.regabiargs"));
        assert!(build_expr_holds(&format!("{os} && {arch}")));
        assert!(build_expr_holds(&format!("plan9 || {arch}")));
        // `&&` binds tighter than `||`: `a || (b && c)`.
        assert!(build_expr_holds(&format!("{os} || race && cgo")));
        assert!(!build_expr_holds(&format!("({os} || race) && cgo")));
        assert!(build_expr_holds(&format!("!({os} && race)")));
        assert!(build_expr_holds("go1.18 && !go1.99"));
        // Malformed input admits nothing.
        assert!(!build_expr_holds(&format!("{os} &&")));
        assert!(!build_expr_holds(&format!("({os}")));
    }

    #[test]
    fn go_build_line_decides_and_plus_build_lines_all_hold() {
        // `internal/race` ships `race.go` and `norace.go`, both declaring
        // `Enabled`; only one belongs to a build.
        assert!(header_constraints_hold(
            "// Copyright\n\n//go:build !race\n\npackage race\n"
        ));
        assert!(!header_constraints_hold(
            "//go:build race\n\npackage race\n"
        ));
        assert!(!header_constraints_hold(
            "// +build ignore\n\npackage main\n"
        ));
        let (os, _) = host();
        assert!(header_constraints_hold(&format!(
            "// +build plan9 {os}\n// +build !race\n\npackage p\n"
        )));
        assert!(!header_constraints_hold(&format!(
            "// +build {os},race\n\npackage p\n"
        )));
        // A constraint below the package clause is a comment, not a constraint.
        assert!(header_constraints_hold("package p\n\n//go:build ignore\n"));
        assert!(header_constraints_hold("package p\n"));
    }
}
