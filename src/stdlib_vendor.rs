//! Vendored Go standard-library source, embedded in the `go` binary so go-rs is
//! self-contained (no `$GOROOT` needed at run time). Packages are vendored here
//! once verified to run on go-rs; until then [`crate::pkg`] falls back to
//! `$GOROOT/src`.

/// Every vendored package: its import path and its source — the package
/// directory's buildable `.go` files with their `package` clauses stripped,
/// joined, the same form [`crate::pkg`] produces from a source directory.
/// Real stdlib source, each verified to run on go-rs.
pub const PACKAGES: &[(&str, &str)] = &[
    ("errors", include_str!("../goroot/errors.go")),
    ("sync", include_str!("../goroot/sync.go")),
    ("sync/atomic", include_str!("../goroot/atomic.go")),
    ("time", include_str!("../goroot/time.go")),
    ("unicode/utf16", include_str!("../goroot/utf16.go")),
    ("cmp", include_str!("../goroot/cmp.go")),
    ("io", include_str!("../goroot/io.go")),
    ("bufio", include_str!("../goroot/bufio.go")),
    ("bytes", include_str!("../goroot/bytes.go")),
    ("iter", include_str!("../goroot/iter.go")),
    ("slices", include_str!("../goroot/slices.go")),
    ("maps", include_str!("../goroot/maps.go")),
    ("math/bits", include_str!("../goroot/bits.go")),
];

/// The source of vendored package `path`, or `None` if it is not vendored.
pub fn source(path: &str) -> Option<String> {
    PACKAGES
        .iter()
        .find(|(p, _)| *p == path)
        .map(|(_, src)| src.to_string())
}
