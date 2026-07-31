//! Offline generator for `docs/reference.html` — the full language reference,
//! rendered with the same cyberpunk HUD chrome as `docs/index.html`. Run before
//! publishing GitHub Pages:
//!
//! ```sh
//! cargo run --bin gen-docs
//! ```
//!
//! Source of truth: the reference corpus in `gors::lsp` (`corpus()`), the exact
//! `Entry` table the editor completion/hover path and `go doc` render from. The
//! static page and the language server therefore never drift — a name is
//! documented here only if the runtime actually recognizes it in `lexer.rs`
//! (keywords), the compiler's type and builtin tables, `host.rs` (the print,
//! composite and stdlib builtins), or the Go source `pkg.rs` links from
//! `goroot/`.

use gors::lsp::Entry;
use std::collections::BTreeSet;
use std::fmt::Write as _;

fn main() {
    let corpus = gors::lsp::corpus();
    let chapters: BTreeSet<&str> = corpus.iter().map(|entry| entry.chapter).collect();

    let page = format!(
        "{head}{body}{foot}",
        head = HEAD,
        body = build_body(corpus),
        foot = FOOT,
    )
    // Stamp the current crate version so the page never falls behind Cargo.toml.
    .replace("__GORS_VERSION__", env!("CARGO_PKG_VERSION"));

    let out = "docs/reference.html";
    if let Err(e) = std::fs::write(out, page) {
        eprintln!("gen-docs: cannot write {out}: {e}");
        std::process::exit(1);
    }
    println!(
        "wrote {out} ({} entries, {} chapters)",
        corpus.len(),
        chapters.len()
    );
}

/// Render one `<section>` per chapter (first-seen order), each holding one
/// `<article class="doc-entry">` per name: an anchored heading, the declaration
/// form in a fenced code block, the description, and a runnable example.
fn build_body(corpus: &[Entry]) -> String {
    let mut chapters: Vec<(&str, Vec<&Entry>)> = Vec::new();
    for entry in corpus {
        match chapters.iter_mut().find(|(c, _)| *c == entry.chapter) {
            Some((_, entries)) => entries.push(entry),
            None => chapters.push((entry.chapter, vec![entry])),
        }
    }

    let mut out = String::new();
    for (chapter, entries) in &chapters {
        let _ = write!(
            out,
            "\n      <section class=\"tutorial-section\" id=\"ch-{slug}\">\n\
             \x20       <h2>{title}</h2>\n",
            slug = slugify(chapter),
            title = html_escape(chapter),
        );
        for (idx, entry) in entries.iter().enumerate() {
            let anchor = format!("doc-{}-{}", slugify(chapter), idx + 1);
            let _ = write!(
                out,
                "        <article class=\"doc-entry\" id=\"{anchor}\">\n\
                 \x20         <h3><a class=\"doc-anchor\" href=\"#{anchor}\">#</a> <code>{name}</code></h3>\n\
                 \x20         <pre><code class=\"lang-go\">{signature}</code></pre>\n\
                 \x20         <p>{doc}</p>\n\
                 \x20         <pre><code class=\"lang-go\">{example}</code></pre>\n\
                 \x20       </article>\n",
                anchor = anchor,
                name = html_escape(entry.name),
                signature = html_escape(entry.signature),
                doc = html_escape(entry.doc),
                example = html_escape(entry.example),
            );
        }
        out.push_str("      </section>\n");
    }
    out
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Lowercase, non-alphanumeric runs collapsed to a single `-`, edges trimmed.
fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

const HEAD: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="color-scheme" content="dark light">
  <meta name="description" content="go-rs — Language reference. Every keyword, type, conversion, builtin, standard-library function, operator, statement and expression form the current go-rs build implements, plus its documented divergences from gc-compiled Go. MIT licensed.">
  <title>go-rs &mdash; Language Reference</title>
  <link rel="preconnect" href="https://fonts.googleapis.com">
  <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
  <link href="https://fonts.googleapis.com/css2?family=Orbitron:wght@400;600;700;900&family=Share+Tech+Mono&display=swap" rel="stylesheet">
  <link rel="stylesheet" href="hud-static.css">
  <link rel="stylesheet" href="tutorial.css">
  <style>
    .tutorial-main { max-width: 76rem; }
    .section-rule { border:none;border-top:1px dashed var(--border);margin:2rem 0; }
    .hub-scheme-strip { border-bottom:1px dashed var(--border);background:color-mix(in srgb, var(--bg-secondary) 85%, transparent);padding:0.55rem 1.5rem 0.65rem;position:relative; }
    .hub-scheme-strip-inner { max-width:76rem;margin:0 auto;display:flex;align-items:center;gap:0.85rem; }
    .hub-scheme-strip .hud-scheme-label { flex:0 0 auto;font-family:'Orbitron',sans-serif;font-size:9px;font-weight:700;letter-spacing:2px;text-transform:uppercase;color:var(--accent);text-align:left; }
    .hub-scheme-strip .scheme-grid { flex:1 1 auto;display:grid;grid-template-columns:repeat(5,minmax(0,1fr));gap:6px; }
    @media (max-width:720px){ .hub-scheme-strip-inner{flex-direction:column;align-items:stretch}.hub-scheme-strip .scheme-grid{grid-template-columns:repeat(2,minmax(0,1fr))} }
    .docs-build-line { margin:0.35rem 0 0;font-family:'Share Tech Mono',ui-monospace,monospace;font-size:11px;color:var(--text-dim);letter-spacing:0.03em;max-width:52rem;opacity:0.75; }
  </style>
</head>
<body>
  <div class="app tutorial-app" id="docsApp">
    <div class="crt-scanline" id="crtH" aria-hidden="true"></div>
    <div class="crt-scanline-v" id="crtV" aria-hidden="true"></div>

    <header class="tutorial-header">
      <div class="tutorial-header-inner">
        <div>
          <h1 class="tutorial-brand">// GO-RS — LANGUAGE REFERENCE</h1>
          <nav class="tutorial-crumbs" aria-label="Breadcrumb">
            <a href="index.html">Docs</a>
            <span class="sep">/</span>
            <span class="current">Language Reference</span>
            <span class="sep">/</span>
            <a href="https://github.com/MenkeTechnologies/go-rs" target="_blank" rel="noopener noreferrer">GitHub</a>
          </nav>
          <p class="docs-build-line">go-rs v__GORS_VERSION__ · Go on fusevm · lex/parse → AST → bytecode → Cranelift JIT · no bespoke VM · no go toolchain · MIT · in active development</p>
        </div>
        <div class="tutorial-toolbar">
          <button type="button" class="btn btn-secondary" id="btnTheme" title="Toggle light/dark">Theme</button>
          <button type="button" class="btn btn-secondary active" id="btnCrt" title="CRT scanline overlay">CRT</button>
          <button type="button" class="btn btn-secondary active" id="btnNeon" title="Neon border pulse">Neon</button>
          <a class="btn btn-secondary" href="index.html">Docs</a>
          <a class="btn btn-secondary" href="https://github.com/MenkeTechnologies/go-rs" target="_blank" rel="noopener noreferrer">GitHub</a>
        </div>
      </div>
    </header>

    <div class="hub-scheme-strip">
      <div class="hub-scheme-strip-inner">
        <span class="hud-scheme-label">// Color scheme</span>
        <div class="scheme-grid" id="hudSchemeGrid"></div>
      </div>
    </div>

    <main class="tutorial-main">
      <h2 class="tutorial-title"><span class="step-hash">&gt;_</span>LANGUAGE REFERENCE</h2>
      <p class="tutorial-subtitle">Every keyword, predeclared identifier, type, conversion, builtin function, standard-library entry, operator, statement form and expression form the current go-rs build implements — each with its declaration form, what the implementation actually does, and a runnable example. The last chapter documents where behaviour diverges from gc-compiled Go. This page is generated from the reference corpus (<code>src/lsp.rs</code>) by the <code>gen-docs</code> binary, so it never drifts from what the runtime, <code>go doc</code> and the language server know: keywords mirror <code>lexer.rs</code>; types, conversions and builtins mirror the compiler's tables; the package chapters mirror the native dispatch tables in <code>host.rs</code> and the Go source linked from <code>goroot/</code>.</p>
"#;

const FOOT: &str = r#"
      <section class="tutorial-section">
        <h2>More</h2>
        <ul>
          <li><strong>Docs</strong> — <a href="index.html">index.html</a> (overview, architecture, examples)</li>
          <li><strong>Engineering report</strong> — <a href="report.html">report.html</a> (value model, status, dependencies)</li>
          <li><strong>Source</strong> — <a href="https://github.com/MenkeTechnologies/go-rs">github.com/MenkeTechnologies/go-rs</a></li>
        </ul>
      </section>
    </main>

  </div>

  <script src="hud-theme.js"></script>
</body>
</html>
"#;
