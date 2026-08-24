//! `error` as an interface type in a type switch and a type assertion.
//!
//! `error` is predeclared, so it never appears in the program's interface
//! declarations — and treating it as the empty interface makes `case error:`
//! match every value, silently, with no compile error anywhere. That is the
//! shape this file exists to catch: the whole switch keeps running, the wrong
//! arm just wins.
//!
//! Every expectation is the verbatim stdout of `go run` on the same source
//! (checked against `go1.27.0 darwin/arm64`). The end-to-end byte gate is
//! `parity-scripts/error_interface.go`; these run the built binary directly, so
//! they need no reference toolchain on the machine.

use std::io::Write;
use std::process::Command;

/// Compile and run `src` through the built `go` binary; return (stdout, success).
fn run(src: &str) -> (String, bool) {
    let mut f = tempfile::Builder::new()
        .suffix(".go")
        .tempfile()
        .expect("temp file");
    f.write_all(src.as_bytes()).expect("write source");

    let out = Command::new(env!("CARGO_BIN_EXE_go"))
        .arg("run")
        .arg(f.path())
        .output()
        .expect("spawn go binary");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.success(),
    )
}

/// `case error:` is method-set containment on `Error() string`, so it takes an
/// `errors.New` value and a type with its own `Error` method and nothing else.
/// A `float64` reaching it is the failure this catches.
#[test]
fn a_case_on_error_tests_the_method_set() {
    let src = r#"package main

import (
	"errors"
	"fmt"
)

type myErr struct{ m string }

func (e myErr) Error() string { return "my:" + e.m }

type stringer interface{ String() string }

type notErr struct{}

func (notErr) String() string { return "ns" }

func kind(i any) string {
	switch v := i.(type) {
	case int:
		return fmt.Sprintf("int:%d", v+1)
	case error:
		return "err:" + v.Error()
	case stringer:
		return "str:" + v.String()
	case string, bool:
		return fmt.Sprintf("sb:%v", v)
	default:
		return fmt.Sprintf("other:%T", v)
	}
}

func main() {
	fmt.Println(kind(1))
	fmt.Println(kind(2.5))
	fmt.Println(kind("s"))
	fmt.Println(kind(true))
	fmt.Println(kind(myErr{"x"}))
	fmt.Println(kind(errors.New("e")))
	fmt.Println(kind(notErr{}))
	fmt.Println(kind([]int{1}))
	fmt.Println(kind(nil))
}
"#;
    let (stdout, ok) = run(src);
    assert!(ok, "program failed; stdout was: {stdout:?}");
    assert_eq!(
        stdout,
        "int:2\nother:float64\nsb:s\nsb:true\nerr:my:x\nerr:e\nstr:ns\nother:[]int\nother:<nil>\n"
    );
}

/// The comma-ok assertion answers the same question, and the single-result form
/// panics with Go's `TypeAssertionError` text — which is *not* one of the
/// `runtime error: ` messages, names a declared interface with its package, and
/// names the missing method rather than its signature.
#[test]
fn an_error_assertion_and_its_conversion_panic() {
    let src = r#"package main

import "fmt"

type myErr struct{ m string }

func (e myErr) Error() string { return "my:" + e.m }

type stringer interface{ String() string }

func try(f func()) {
	defer func() { fmt.Println("rec:", recover()) }()
	f()
}

func main() {
	var a any = myErr{"y"}
	e, ok := a.(error)
	fmt.Println(e, ok)
	var b any = 7
	_, ok2 := b.(error)
	fmt.Println(ok2)

	try(func() { _ = b.(error) })
	try(func() { _ = b.(string) })
	try(func() { _ = b.(stringer) })
	var n any
	try(func() { _ = n.(error) })
	try(func() { _ = n.(int) })
}
"#;
    let (stdout, ok) = run(src);
    assert!(ok, "program failed; stdout was: {stdout:?}");
    assert_eq!(
        stdout,
        "my:y true\n\
         false\n\
         rec: interface conversion: int is not error: missing method Error\n\
         rec: interface conversion: interface {} is int, not string\n\
         rec: interface conversion: int is not main.stringer: missing method String\n\
         rec: interface conversion: interface is nil, not error\n\
         rec: interface conversion: interface {} is nil, not int\n"
    );
}
