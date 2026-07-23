//! Language Server Protocol over stdio (`go --lsp`).
//!
//! Self-contained and read-only: diagnostics come from the same `parser::parse`
//! the runtime uses (a syntax error maps to the reported line); hover and
//! completion draw on the keyword / type / IO corpus below. No output ever
//! reaches the terminal — JSON-RPC on stdio only. Structure follows the sibling
//! `-rs` frontends' `lsp.rs`.

use std::collections::HashMap;

use lsp_server::{Connection, ErrorCode, ExtractError, Message, Request, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
    PublishDiagnostics,
};
use lsp_types::request::{Completion, HoverRequest, Request as _};
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse,
    Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, Hover, HoverContents, HoverParams, HoverProviderCapability,
    MarkupContent, MarkupKind, Position, PublishDiagnosticsParams, Range, ServerCapabilities,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions, Uri,
};

/// The keyword / type / IO corpus: (name, chapter, one-line doc, example).
/// Single source of truth for LSP completion and hover, and for the offline
/// `docs/reference.html` generator. Every entry mirrors a surface the current
/// go-rs build actually recognizes:
///   * "Keyword" → a reserved word in `lexer.rs` (`keyword_or_ident`).
///   * "Type" → an identifier go-rs recognizes in declaration position; the
///     runtime is dynamically typed on the fusevm value model, so the
///     annotation drives `/` truncation and comparison-op choice but does not
///     gate execution.
///   * "IO" → the `fmt` methods and builtins lowered to the print builtins in
///     `host.rs` (`GPRINTLN` / `GPRINT` / `GPRINTF` / `GEPRINTLN` / `GEPRINT`).
const CORPUS: &[(&str, &str, &str, &str)] = &[
    // ── Keyword (lexer keyword_or_ident) ──
    (
        "package",
        "Keyword",
        "declare the package; go-rs runs `func main` of `package main`",
        "package main",
    ),
    (
        "import",
        "Keyword",
        "import a package; single (`import \"fmt\"`) or grouped `( … )`",
        "import \"fmt\"",
    ),
    (
        "func",
        "Keyword",
        "declare a function; `func main()` is the entry point",
        "func add(a int, b int) int { return a + b }",
    ),
    (
        "var",
        "Keyword",
        "declare a variable with an optional type and initializer",
        "var n int = 42",
    ),
    (
        "const",
        "Keyword",
        "declare a constant (recognized by the lexer)",
        "const pi = 3.14159",
    ),
    (
        "type",
        "Keyword",
        "declare a named type; `type T struct { … }` defines a struct",
        "type Point struct { x int; y int }",
    ),
    (
        "struct",
        "Keyword",
        "a struct type: a fixed set of named fields with value semantics",
        "type Point struct { x, y int }",
    ),
    (
        "interface",
        "Keyword",
        "an interface type: a method set; any type with those methods satisfies it (dynamic dispatch)",
        "type Shape interface { area() int }",
    ),
    (
        "if",
        "Keyword",
        "conditional branch with an optional init clause: `if [init;] cond { … }`",
        "if x := f(); x > 0 { fmt.Println(x) }",
    ),
    (
        "else",
        "Keyword",
        "fallback branch of an `if`; chains as `else if`",
        "if x > 0 { } else { fmt.Println(\"non-pos\") }",
    ),
    (
        "for",
        "Keyword",
        "the loop: three-clause, condition-only, or infinite",
        "for i := 0; i < 3; i++ { fmt.Println(i) }",
    ),
    (
        "range",
        "Keyword",
        "iterate a collection in a `for … range` (reserved; next wave)",
        "for i := range xs { }",
    ),
    (
        "return",
        "Keyword",
        "return from the current function with an optional value",
        "return a + b",
    ),
    (
        "break",
        "Keyword",
        "exit the nearest `for` loop",
        "for { if done { break } }",
    ),
    (
        "continue",
        "Keyword",
        "skip to the next iteration of the nearest `for` loop",
        "for i := 0; i < 5; i++ { if i%2 == 0 { continue } }",
    ),
    ("true", "Keyword", "the boolean literal true", "ok := true"),
    (
        "false",
        "Keyword",
        "the boolean literal false",
        "ok := false",
    ),
    (
        "go",
        "Keyword",
        "spawn a goroutine: run a function concurrently on the cooperative scheduler",
        "go worker(jobs, results)",
    ),
    (
        "chan",
        "Keyword",
        "a channel type: `chan T` carries values of type T between goroutines",
        "ch := make(chan int, 8)",
    ),
    (
        "select",
        "Keyword",
        "wait on multiple channel operations; runs a ready case, else `default`, else blocks",
        "select { case v := <-ch: use(v); default: }",
    ),
    (
        "defer",
        "Keyword",
        "schedule a call to run at function return (LIFO); arguments are evaluated now",
        "defer f.Close()",
    ),
    (
        "switch",
        "Keyword",
        "multi-way branch: tagged (`switch x`) or expression (`switch`) form; first matching case runs, no implicit fallthrough",
        "switch { case n < 0: neg(); default: pos() }",
    ),
    // ── Type (declaration-position type names) ──
    (
        "int",
        "Type",
        "machine integer; `int / int` truncates toward zero",
        "var n int = 7",
    ),
    (
        "int64",
        "Type",
        "64-bit signed integer",
        "var big int64 = 9000000000",
    ),
    (
        "float64",
        "Type",
        "64-bit float; `%v` prints whole values without a fraction (3, not 3.0)",
        "var d float64 = 3.0   // fmt.Println(d) => 3",
    ),
    ("float32", "Type", "32-bit float", "var f float32 = 1.5"),
    (
        "string",
        "Type",
        "string; `+` concatenates, `<`/`==` order lexicographically (host numeric hook)",
        "s := \"go\" + \"-rs\"   // => 'go-rs'",
    ),
    ("bool", "Type", "boolean (`true` / `false`)", "ok := 3 < 5"),
    ("byte", "Type", "alias for uint8", "var b byte = 65"),
    (
        "rune",
        "Type",
        "alias for int32; a Unicode code point",
        "var r rune = 'A'",
    ),
    // ── IO (fmt + builtins) ──
    (
        "Println",
        "IO",
        "fmt.Println(a…): print operands space-separated, then a newline, to stdout",
        "fmt.Println(\"hello\", 42)",
    ),
    (
        "Print",
        "IO",
        "fmt.Print(a…): print operands (space between non-strings) with no newline",
        "fmt.Print(\"x = \", 1)",
    ),
    (
        "Printf",
        "IO",
        "fmt.Printf(format, a…): format with %v %d %s %f %t %q %%",
        "fmt.Printf(\"%d and %s\\n\", 42, \"hi\")",
    ),
    (
        "Sprintf",
        "IO",
        "fmt.Sprintf(format, a…): like Printf but returns the string instead of printing (also Sprint / Sprintln)",
        "s := fmt.Sprintf(\"%d-%s\", 42, \"go\")",
    ),
    (
        "fmt",
        "IO",
        "the fmt package; go-rs wires Println / Print / Printf",
        "import \"fmt\"",
    ),
    (
        "println",
        "IO",
        "builtin println(a…): space-separated, trailing newline, to stderr",
        "println(\"debug\", x)",
    ),
    (
        "print",
        "IO",
        "builtin print(a…): to stderr with no newline",
        "print(\"debug\")",
    ),
    // ── Builtin (predeclared functions over composite types) ──
    (
        "make",
        "Builtin",
        "make([]T, n) allocates a zeroed slice; make(map[K]V) an empty map",
        "xs := make([]int, 3); m := make(map[string]int)",
    ),
    (
        "len",
        "Builtin",
        "len(x): the number of elements in a slice/map, or bytes in a string",
        "n := len([]int{1, 2, 3})   // 3",
    ),
    (
        "cap",
        "Builtin",
        "cap(x): the capacity of a slice (its length in go-rs)",
        "c := cap(make([]int, 4))",
    ),
    (
        "append",
        "Builtin",
        "append(s, elems…): extend a slice, returning the result",
        "xs = append(xs, 4, 5)",
    ),
    (
        "delete",
        "Builtin",
        "delete(m, k): remove key k from map m",
        "delete(m, \"a\")",
    ),
    (
        "close",
        "Builtin",
        "close(ch): close a channel; further receives yield the zero value",
        "close(done)",
    ),
    (
        "panic",
        "Builtin",
        "panic(v): stop normal flow and unwind, running deferred calls; a recover() may stop it",
        "panic(\"unreachable\")",
    ),
    (
        "recover",
        "Builtin",
        "recover(): inside a deferred call, stop a panic and return its value (nil if none)",
        "defer func() { recover() }()",
    ),
    // ── Package (standard library) ──
    (
        "strings",
        "Package",
        "string helpers: ToUpper/ToLower/Contains/HasPrefix/HasSuffix/TrimSpace/Split/Join/Repeat/Index/ReplaceAll/Fields",
        "strings.Join(strings.Split(\"a,b\", \",\"), \"-\")",
    ),
    (
        "strconv",
        "Package",
        "string↔number conversions: Itoa (int→string), Atoi (string→int)",
        "s := strconv.Itoa(42); n := strconv.Atoi(\"7\")",
    ),
];

/// The corpus, exposed for offline doc generation.
pub fn corpus() -> &'static [(&'static str, &'static str, &'static str, &'static str)] {
    CORPUS
}

/// Render `go doc [name]` from the corpus. With a name, print that one entry's
/// category, description, and example (case-sensitive exact match first, then a
/// case-insensitive fallback). Without a name, print the full index grouped by
/// category. Returns the text to print, or an error string if `name` is unknown.
pub fn doc(name: Option<&str>) -> Result<String, String> {
    let Some(name) = name else {
        // Full index, grouped by category in first-seen order.
        let mut out = String::from("go-rs reference — documented surfaces\n");
        let mut cats: Vec<&str> = Vec::new();
        for (_, cat, _, _) in CORPUS {
            if !cats.contains(cat) {
                cats.push(cat);
            }
        }
        for cat in cats {
            out.push_str(&format!("\n{cat}\n"));
            for (n, c, doc, _) in CORPUS {
                if c == &cat {
                    out.push_str(&format!("  {n:<10} {doc}\n"));
                }
            }
        }
        return Ok(out);
    };

    let entry = CORPUS.iter().find(|(n, _, _, _)| *n == name).or_else(|| {
        CORPUS
            .iter()
            .find(|(n, _, _, _)| n.eq_ignore_ascii_case(name))
    });
    match entry {
        Some((n, cat, doc, example)) => Ok(format!(
            "{n}  ({cat})\n\n    {doc}\n\nexample:\n    {example}\n"
        )),
        None => Err(format!(
            "go-rs: no documentation for `{name}` (try `go doc` for the index)"
        )),
    }
}

/// Open document text keyed by URI, kept current from the sync notifications so
/// hover can look up the identifier under the cursor.
type Docs = HashMap<String, String>;

/// Entry point for `go --lsp`.
pub fn run() -> Result<(), String> {
    spawn_orphan_guard();
    let (conn, io_threads) = Connection::stdio();
    let (init_id, _params) = conn
        .initialize_start()
        .map_err(|e| format!("lsp initialize: {e}"))?;
    let init_result = serde_json::json!({
        "capabilities": server_capabilities(),
        "serverInfo": { "name": "go-rs", "version": env!("CARGO_PKG_VERSION") },
    });
    conn.sender
        .send(Response::new_ok(init_id, init_result).into())
        .map_err(|e| format!("lsp send: {e}"))?;

    let mut docs: Docs = HashMap::new();
    for msg in &conn.receiver {
        match msg {
            Message::Request(req) => {
                if conn
                    .handle_shutdown(&req)
                    .map_err(|e| format!("lsp shutdown: {e}"))?
                {
                    break;
                }
                dispatch_request(&conn, &docs, req);
            }
            Message::Notification(not) => dispatch_notification(&conn, &mut docs, not),
            Message::Response(_) => {}
        }
    }
    drop(conn);
    io_threads.join().map_err(|_| "lsp io join".to_string())?;
    Ok(())
}

fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::FULL),
                ..Default::default()
            },
        )),
        completion_provider: Some(CompletionOptions {
            resolve_provider: Some(false),
            ..Default::default()
        }),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        ..Default::default()
    }
}

fn handle<P, R>(conn: &Connection, req: Request, f: impl FnOnce(P) -> R)
where
    P: serde::de::DeserializeOwned,
    R: serde::Serialize,
{
    let method = req.method.clone();
    let id = req.id.clone();
    match req.extract::<P>(&method) {
        Ok((id, params)) => {
            let value = serde_json::to_value(f(params)).unwrap_or(serde_json::Value::Null);
            let _ = conn.sender.send(Response::new_ok(id, value).into());
        }
        Err(ExtractError::JsonError { error, .. }) => {
            let _ = conn.sender.send(
                Response::new_err(id, ErrorCode::InvalidParams as i32, error.to_string()).into(),
            );
        }
        Err(ExtractError::MethodMismatch(_)) => unreachable!("method matched before extract"),
    }
}

fn dispatch_request(conn: &Connection, docs: &Docs, req: Request) {
    match req.method.as_str() {
        Completion::METHOD => handle(conn, req, |_p: CompletionParams| completions()),
        HoverRequest::METHOD => handle(conn, req, |p: HoverParams| hover(docs, &p)),
        _ => {
            let _ = conn.sender.send(
                Response::new_err(req.id, ErrorCode::MethodNotFound as i32, "unhandled".into())
                    .into(),
            );
        }
    }
}

fn dispatch_notification(conn: &Connection, docs: &mut Docs, not: lsp_server::Notification) {
    match not.method.as_str() {
        DidOpenTextDocument::METHOD => {
            if let Ok(p) = serde_json::from_value::<DidOpenTextDocumentParams>(not.params) {
                let uri = p.text_document.uri;
                docs.insert(uri.as_str().to_string(), p.text_document.text.clone());
                publish_diagnostics(conn, &uri, &p.text_document.text);
            }
        }
        DidChangeTextDocument::METHOD => {
            if let Ok(p) = serde_json::from_value::<DidChangeTextDocumentParams>(not.params) {
                if let Some(change) = p.content_changes.into_iter().last() {
                    let uri = p.text_document.uri;
                    docs.insert(uri.as_str().to_string(), change.text.clone());
                    publish_diagnostics(conn, &uri, &change.text);
                }
            }
        }
        DidCloseTextDocument::METHOD => {
            if let Ok(p) = serde_json::from_value::<DidCloseTextDocumentParams>(not.params) {
                let uri = p.text_document.uri;
                docs.remove(uri.as_str());
                publish_diagnostics(conn, &uri, "");
            }
        }
        _ => {}
    }
}

fn completions() -> CompletionResponse {
    let items = CORPUS
        .iter()
        .map(|(name, chapter, doc, _example)| CompletionItem {
            label: name.to_string(),
            kind: Some(match *chapter {
                "Keyword" => CompletionItemKind::KEYWORD,
                "Type" => CompletionItemKind::CLASS,
                _ => CompletionItemKind::METHOD,
            }),
            detail: Some((*doc).to_string()),
            ..Default::default()
        })
        .collect();
    CompletionResponse::Array(items)
}

/// Hover: look up the identifier under the cursor in the corpus and render its
/// chapter, doc, and example. Falls back to a short banner when the cursor is
/// not on a known name.
fn hover(docs: &Docs, params: &HoverParams) -> Hover {
    let pos = params.text_document_position_params.position;
    let uri = params
        .text_document_position_params
        .text_document
        .uri
        .as_str();
    let word = docs
        .get(uri)
        .and_then(|text| word_at(text, pos))
        .unwrap_or_default();

    let matches: Vec<&(&str, &str, &str, &str)> =
        CORPUS.iter().filter(|(name, ..)| *name == word).collect();

    let body = if matches.is_empty() {
        "**go-rs** — Go on the fusevm bytecode VM + Cranelift JIT.".to_string()
    } else {
        let mut out = String::new();
        for (name, chapter, doc, example) in matches {
            out.push_str(&format!(
                "**`{name}`** — _{chapter}_\n\n{doc}\n\n```go\n{example}\n```\n\n"
            ));
        }
        out.trim_end().to_string()
    };

    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: body,
        }),
        range: None,
    }
}

/// Extract the identifier (`[A-Za-z0-9_]+`) spanning the given position, if any.
fn word_at(text: &str, pos: Position) -> Option<String> {
    let line = text.lines().nth(pos.line as usize)?;
    let chars: Vec<char> = line.chars().collect();
    let col = (pos.character as usize).min(chars.len());
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';

    let mut start = col;
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    let mut end = col;
    while end < chars.len() && is_word(chars[end]) {
        end += 1;
    }
    if start == end {
        return None;
    }
    Some(chars[start..end].iter().collect())
}

fn publish_diagnostics(conn: &Connection, uri: &Uri, text: &str) {
    let params = PublishDiagnosticsParams {
        uri: uri.clone(),
        diagnostics: compute_diagnostics(text),
        version: None,
    };
    let not = lsp_server::Notification::new(PublishDiagnostics::METHOD.to_string(), params);
    let _ = conn.sender.send(not.into());
}

/// Parse the whole document with the runtime's own parser; a syntax error maps
/// to a single diagnostic on the line named in its `on line N` / `(line N)`
/// suffix.
fn compute_diagnostics(text: &str) -> Vec<Diagnostic> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    match crate::parse(text) {
        Ok(_) => Vec::new(),
        Err(e) => {
            let line = parse_error_line(&e).saturating_sub(1);
            vec![Diagnostic {
                range: Range {
                    start: Position { line, character: 0 },
                    end: Position {
                        line,
                        character: 200,
                    },
                },
                severity: Some(DiagnosticSeverity::ERROR),
                message: e,
                ..Default::default()
            }]
        }
    }
}

/// Extract the (1-based) line number from a go-rs parser error, which embeds it
/// as `… on line N` or `… (line N)`. Defaults to line 1 when no marker is present.
fn parse_error_line(e: &str) -> u32 {
    let after = e
        .rsplit_once("on line ")
        .map(|(_, rest)| rest)
        .or_else(|| e.rsplit_once("(line ").map(|(_, rest)| rest));
    after
        .and_then(|rest| {
            rest.split(|c: char| !c.is_ascii_digit())
                .find(|s| !s.is_empty())
        })
        .and_then(|n| n.parse().ok())
        .unwrap_or(1)
}

/// Exit if reparented to pid 1 (the editor died) so we never leak.
fn spawn_orphan_guard() {
    std::thread::spawn(|| {
        #[cfg(target_os = "linux")]
        // SAFETY: prctl(PR_SET_PDEATHSIG, ...) only registers a signal disposition.
        unsafe {
            libc::prctl(
                libc::PR_SET_PDEATHSIG,
                libc::SIGKILL as libc::c_ulong,
                0,
                0,
                0,
            );
        }
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            // SAFETY: getppid takes no arguments and never fails.
            if unsafe { libc::getppid() } == 1 {
                std::process::exit(0);
            }
        }
    });
}
