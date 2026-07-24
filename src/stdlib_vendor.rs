//! Vendored Go standard-library source, embedded in the `go` binary so go-rs is
//! self-contained (no `$GOROOT` needed at run time). Packages are vendored here
//! once verified to run on go-rs; until then [`crate::pkg`] falls back to
//! `$GOROOT/src`.

/// The concatenated source of vendored package `path`, or `None` if not
/// vendored. Each entry is the package directory's buildable `.go` files with
/// their `package` clauses stripped, joined — the same form [`crate::pkg`]
/// produces from a source directory.
pub fn source(path: &str) -> Option<String> {
    let text: &str = match path {
        // Vendored packages (verified to run on go-rs). Real stdlib source.
        "errors" => include_str!("../goroot/errors.go"),
        "unicode/utf16" => include_str!("../goroot/utf16.go"),
        "cmp" => include_str!("../goroot/cmp.go"),
        _ => return None,
    };
    Some(text.to_string())
}
