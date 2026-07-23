//! The `go version` banner string.

/// The Go language level go-rs targets — reported by `go version` so tools that
/// parse the version line read a familiar level, followed by the real engine so
/// nothing is misrepresented as the `go` toolchain.
pub const GO_COMPAT_VERSION: &str = "1.22";

/// The engine name — go-rs is its own runtime (like `gc` is the reference Go
/// compiler).
pub const GO_ENGINE: &str = "go-rs";

/// The host `arch-os` string.
pub fn platform() -> String {
    format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS)
}

/// The `go version` banner. Names the targeted language level, then the real
/// engine, its crate version, and the host triple.
pub fn version_banner() -> String {
    format!(
        "go version go{} ({} {}) [{}]",
        GO_COMPAT_VERSION,
        GO_ENGINE,
        env!("CARGO_PKG_VERSION"),
        platform()
    )
}
