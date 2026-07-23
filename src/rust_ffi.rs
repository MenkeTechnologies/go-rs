//! Go wiring for inline Rust FFI (`rust { ... }` blocks).
//!
//! The heavy lifting lives in fusevm: [`fusevm::RustSugar`] scans and rewrites
//! the block at the source level, and `fusevm::ffi` compiles / loads / marshals
//! it. This module only supplies the Go-flavored [`fusevm::RustSugar`] config
//! and the desugar entry [`crate::parse`] calls before lexing. The emitted
//! `__rust_compile(...)` call and every exported bareword are resolved in
//! [`crate::compiler`] (the `GFFI_COMPILE` / `GFFI_CALL` builtins) and executed
//! by [`crate::host`].
//!
//! A `rust { ... }` block appears as a statement inside a function body; the
//! desugar replaces it in place with a `__rust_compile("<base64>", <line>);`
//! statement. The explicit trailing `;` is a valid Go empty-safe terminator (Go
//! usually omits it via automatic semicolon insertion, but tolerates it), so the
//! parser sees an ordinary expression statement.

use fusevm::RustSugar;

/// Emit the Go statement a `rust { ... }` block desugars to: a call to the
/// `__rust_compile` builtin carrying the base64-encoded block body and its line,
/// terminated by `;`. base64's alphabet (`A-Za-z0-9+/=`) has no `"` or `\`, so
/// it needs no escaping inside the double-quoted Go string literal.
fn emit(b64: &str, line: usize) -> String {
    format!("__rust_compile(\"{b64}\", {line});")
}

/// Go desugar config. Line comments are `//`, block comments `/* */`.
/// `newline_boundary` is `true` so a `rust { ... }` block starting a statement
/// line is recognized; `{`/`}`/`;` are boundaries too.
pub const SUGAR: RustSugar = RustSugar {
    keyword: "rust",
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    newline_boundary: true,
    emit,
};

/// Rewrite every `rust { ... }` block in Go source into a `__rust_compile(...)`
/// statement, before lexing. No-op when the source has no `rust` token.
pub fn desugar(src: &str) -> String {
    SUGAR.desugar(src)
}

#[cfg(test)]
mod tests {
    #[test]
    fn desugars_block_inside_main() {
        let src = "package main\nfunc main() {\n\trust { pub extern \"C\" fn add(a: i64, b: i64) -> i64 { a + b } }\n\tprintln(add(2, 3))\n}\n";
        let out = super::desugar(src);
        assert!(out.contains("__rust_compile("), "no builtin call: {out}");
        assert!(!out.contains("pub extern"), "Rust body leaked: {out}");
        assert!(
            out.contains("println(add(2, 3))"),
            "trailing code lost: {out}"
        );
    }

    #[test]
    fn leaves_ordinary_go_untouched() {
        let src = "package main\nfunc main() {\n\tprintln(41 + 1)\n}\n";
        assert_eq!(super::desugar(src), src);
    }
}
