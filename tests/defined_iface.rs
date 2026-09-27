//! A defined non-struct type (`type Celsius float64`) inside an interface.
//!
//! A defined type is represented exactly like its base, so before it was boxed
//! on its way into an interface a `Celsius` in an `any` was indistinguishable
//! from a `float64`: a method called through the interface answered `<nil>`,
//! `case Celsius:` never matched, and `fmt` never called `String()`. Nothing
//! failed loudly — the wrong line just printed.
//!
//! Every expectation is the verbatim stdout of `go run` on the same source
//! (go1.27.1 darwin/arm64). The end-to-end byte gates are
//! `parity-scripts/defined_types_in_interfaces.go` and
//! `parity-scripts/stringer_verbs_and_elements.go`; these run the built binary
//! directly, so they need no reference toolchain on the machine.

use std::io::Write;
use std::process::Command;

fn run(src: &str) -> String {
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
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Dynamic dispatch, assertion and type switch all see the defined type.
#[test]
fn methods_and_type_tests_reach_the_defined_type() {
    let src = r#"package main

import "fmt"

type Celsius float64

func (c Celsius) String() string { return fmt.Sprintf("%.1fC", float64(c)) }

type Code int

type Shower interface{ String() string }

func show(s Shower) string { return s.String() }

func kind(v any) string {
	switch x := v.(type) {
	case Celsius:
		return fmt.Sprint("celsius ", float64(x)+1)
	case Code:
		return fmt.Sprint("code ", int(x)*2)
	case int:
		return "int"
	}
	return "other"
}

func main() {
	fmt.Println(show(Celsius(21.5)))
	var x any = Celsius(1)
	if s, ok := x.(fmt.Stringer); ok {
		fmt.Println("stringer", s.String())
	}
	fmt.Println(kind(Celsius(20)), kind(Code(5)), kind(7))
	c, ok := any(Code(3)).(Code)
	fmt.Println(c+1, ok)
	_, isInt := any(Code(3)).(int)
	fmt.Println(isInt)
}
"#;
    assert_eq!(
        run(src),
        "21.5C\nstringer 1.0C\ncelsius 21 code 10 int\n4 true\nfalse\n"
    );
}

/// An interface value compares by dynamic type, then value — as a map key too.
#[test]
fn equality_and_map_keys_compare_the_type() {
    let src = r#"package main

import "fmt"

type Code int

func main() {
	var a any = Code(3)
	fmt.Printf("%T %v\n", a, a)
	fmt.Println(a == Code(3), a == 3, a != Code(4))
	m := map[any]string{Code(1): "code", 1: "int"}
	fmt.Println(len(m), m[Code(1)], m[1])
}
"#;
    assert_eq!(run(src), "main.Code 3\ntrue false true\n2 code int\n");
}

/// `fmt` calls `String()` only under the text verbs, and on slice elements.
#[test]
fn fmt_uses_the_method_under_text_verbs_and_on_elements() {
    let src = r#"package main

import (
	"errors"
	"fmt"
)

type Color int

func (c Color) String() string { return [...]string{"R", "G", "B"}[c] }

type ErrCode string

func (e ErrCode) Error() string { return "code:" + string(e) }

func main() {
	fmt.Println(Color(2), []Color{0, 1})
	fmt.Printf("%v|%s|%d|%q|%#v\n", Color(1), Color(1), Color(1), Color(1), Color(1))
	fmt.Println([]any{Color(2), "s"})
	var err error = ErrCode("neg")
	var ec ErrCode
	fmt.Println(err, errors.As(err, &ec), ec)
}
"#;
    assert_eq!(
        run(src),
        "B [R G]\nG|G|1|\"G\"|1\n[B s]\ncode:neg true code:neg\n"
    );
}
