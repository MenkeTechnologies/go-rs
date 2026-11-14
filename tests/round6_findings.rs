//! Regressions for the sixth round of differential testing against the
//! reference `go`: how an unrecovered panic prints its value. Each expected
//! text is the reference's own stderr up to the goroutine dump, which go-rs
//! does not reproduce, so the tests need no `go` toolchain.

use std::process::Command;

/// Run Go source from a temporary file; return (stdout, stderr, exit code).
fn run_src(src: &str) -> (String, String, i32) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("main.go");
    std::fs::write(&path, src).expect("write source");
    let out = Command::new(env!("CARGO_BIN_EXE_go"))
        .arg("run")
        .arg(&path)
        .output()
        .expect("spawn go binary");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

/// `panic(nil)` panics with a `*runtime.PanicNilError`, whose `Error()` text
/// carries the `runtime error: ` prefix.
#[test]
fn panic_nil_prints_runtime_error() {
    let (out, err, code) = run_src(
        r#"package main

import "fmt"

func main() {
	fmt.Println("before")
	panic(nil)
}
"#,
    );
    assert_eq!(out, "before\n");
    assert_eq!(
        err,
        "panic: runtime error: panic called with nil argument\n"
    );
    assert_eq!(code, 2);
}

/// A value of a defined type over a basic kind, with no `Error` / `String`
/// method, is printed by `printanycustomtype` as `main.T(v)`.
#[test]
fn panic_defined_number_prints_type() {
    let (_, err, code) = run_src(
        r#"package main

type Celsius float64

func main() {
	panic(Celsius(0.1))
}
"#,
    );
    assert_eq!(err, "panic: main.Celsius(0.1)\n");
    assert_eq!(code, 2);

    let (_, err, _) = run_src(
        r#"package main

type Flag bool

func main() {
	panic(Flag(true))
}
"#,
    );
    assert_eq!(err, "panic: main.Flag(true)\n");

    let (_, err, _) = run_src(
        r#"package main

type U8 uint8

func main() {
	panic(U8(200))
}
"#,
    );
    assert_eq!(err, "panic: main.U8(200)\n");
}

/// A defined string type is quoted inside its type name, and a newline in it
/// is followed by a tab (`printindented`).
#[test]
fn panic_defined_string_prints_type_and_indents() {
    let (_, err, code) = run_src(
        r#"package main

type Name string

func main() {
	panic(Name("a\nb"))
}
"#,
    );
    assert_eq!(err, "panic: main.Name(\"a\n\tb\")\n");
    assert_eq!(code, 2);
}

/// A plain string, an `error`'s `Error()` and a `Stringer`'s `String()` are
/// printed by `printindented`: every newline is followed by a tab.
#[test]
fn panic_multiline_text_is_indented() {
    let (_, err, _) = run_src(
        r#"package main

func main() {
	panic("s\nx")
}
"#,
    );
    assert_eq!(err, "panic: s\n\tx\n");

    let (_, err, _) = run_src(
        r#"package main

import "errors"

func main() {
	panic(errors.New("a\nb"))
}
"#,
    );
    assert_eq!(err, "panic: a\n\tb\n");

    let (_, err, _) = run_src(
        r#"package main

type Wd int

func (w Wd) String() string { return "W\nd" }

func main() {
	panic(Wd(3))
}
"#,
    );
    assert_eq!(err, "panic: W\n\td\n");
}

/// A basic type's own name is not a custom type: `uint8` prints bare.
#[test]
fn panic_basic_sized_int_prints_bare() {
    let (_, err, _) = run_src(
        r#"package main

func main() {
	panic(uint8(3))
}
"#,
    );
    assert_eq!(err, "panic: 3\n");
}
