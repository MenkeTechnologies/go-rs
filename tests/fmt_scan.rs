//! `fmt`'s scanning family — Go's own scan.go, vendored as
//! `goroot/fmt_scan.go` — run through the built `go` binary: the stdin
//! scanners (`Scan`, `Scanln`, `Scanf`) with input piped in, which the
//! byte-parity corpus cannot feed, and the string and reader forms with the
//! language support they lean on (`case *int64:` in a type switch, an
//! embedded interface's methods, `&err` of an interface variable, an array
//! sized by another package's constant).
//!
//! Every expected string here is what the reference `go run` prints for the
//! same program and input.

use std::io::Write;
use std::process::{Command, Stdio};

/// Compile and run `src` with `stdin` piped to it and no Go toolchain to fall
/// back on (`fmt`'s scanner and the `io`, `os`, `errors` and `unicode/utf8`
/// sources it links must all come from the binary); return (output, success).
fn run_with_stdin(src: &str, stdin: &[u8]) -> (String, bool) {
    let mut f = tempfile::Builder::new()
        .suffix(".go")
        .tempfile()
        .expect("temp file");
    f.write_all(src.as_bytes()).expect("write source");
    let mut child = Command::new(env!("CARGO_BIN_EXE_go"))
        .arg("run")
        .arg(f.path())
        .env("GOROOT", "/nonexistent")
        .env("GO_RS_HOME", "/nonexistent")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn go binary");
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(stdin)
        .expect("write stdin");
    let out = child.wait_with_output().expect("collect child output");
    (
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr),
        out.status.success(),
    )
}

fn assert_stdout(src: &str, stdin: &[u8], expected: &str) {
    let (out, ok) = run_with_stdin(src, stdin);
    assert!(ok, "program failed; output was: {out:?}");
    assert_eq!(out, expected);
}

#[test]
fn scan_reads_stdin_across_lines() {
    // `fmt.Scan` treats newlines as space: a count, then that many values
    // spread over several lines, then EOF once the input is drained.
    let src = r#"package main

import "fmt"

func main() {
	var n int
	fmt.Scan(&n)
	total := 0
	for i := 0; i < n; i++ {
		var v int
		fmt.Scan(&v)
		total += v
	}
	var rest string
	k, err := fmt.Scan(&rest)
	fmt.Println(total, k, err)
}
"#;
    assert_stdout(src, b"3\n10 20\n  30\n", "60 0 EOF\n");
}

#[test]
fn scanf_and_scanln_consume_one_line_each() {
    let src = r#"package main

import "fmt"

func main() {
	var name string
	var age int
	k, err := fmt.Scanf("%s %d\n", &name, &age)
	fmt.Println(k, err, name, age)
	var a, b string
	k, err = fmt.Scanln(&a, &b)
	fmt.Println(k, err, a, b)
	k, err = fmt.Scanln(&a, &b)
	fmt.Println(k, err, a)
}
"#;
    assert_stdout(
        src,
        b"bob 42\nx y\nonly\n",
        "2 <nil> bob 42\n2 <nil> x y\n1 unexpected newline only\n",
    );
}

#[test]
fn sscan_fills_every_basic_pointer_kind() {
    // One operand per `case *T:` of scanOne's type switch, sized integers
    // included: `9000000000` fits an int64, and `300` overflows an int8.
    let src = r#"package main

import "fmt"

func main() {
	var (
		i   int
		i8  int8
		i64 int64
		u16 uint16
		f   float64
		f32 float32
		s   string
		b   bool
		bs  []byte
	)
	n, err := fmt.Sscan("42 -7 9000000000\n65535 3.25 1.5 word true bytes", &i, &i8, &i64, &u16, &f, &f32, &s, &b, &bs)
	fmt.Println(n, err, i, i8, i64, u16, f, f32, s, b, string(bs))
	n, err = fmt.Sscan("300", &i8)
	fmt.Println(n, err, i8)
	var x, y, z int
	n, err = fmt.Sscan("0x1F 0b101 1_000", &x, &y, &z)
	fmt.Println(n, err, x, y, z)
	n, err = fmt.Sscan("12", i)
	fmt.Println(n, err)
}
"#;
    assert_stdout(
        src,
        b"",
        "9 <nil> 42 -7 9000000000 65535 3.25 1.5 word true bytes\n\
         0 integer overflow on token 300 -7\n\
         3 <nil> 31 5 1000\n\
         0 type not a pointer: int\n",
    );
}

#[test]
fn sscanf_verbs_widths_and_mismatches() {
    let src = r#"package main

import "fmt"

func main() {
	var hx, oc, d1, d2 int
	n, err := fmt.Sscanf("ff 17", "%x %o", &hx, &oc)
	fmt.Println(n, err, hx, oc)
	n, err = fmt.Sscanf("12345", "%2d%3d", &d1, &d2)
	fmt.Println(n, err, d1, d2)
	var r rune
	n, err = fmt.Sscanf("Zq", "%c", &r)
	fmt.Println(n, err, string(r))
	var q string
	n, err = fmt.Sscanf(`"a\tb"`, "%q", &q)
	fmt.Printf("%d %v %q\n", n, err, q)
	var pct int
	n, err = fmt.Sscanf("50%", "%d%%", &pct)
	fmt.Println(n, err, pct)
	n, err = fmt.Sscanf("7 8", "%d,%d", &d1, &d2)
	fmt.Println(n, err, d1)
	n, err = fmt.Sscanf("1 2", "%d", &d1, &d2)
	fmt.Println(n, err)
	n, err = fmt.Sscanf("1", "%d %d", &d1)
	fmt.Println(n, err)
}
"#;
    assert_stdout(
        src,
        b"",
        "2 <nil> 255 15\n\
         2 <nil> 12 345\n\
         1 <nil> Z\n\
         1 <nil> \"a\\tb\"\n\
         1 <nil> 50\n\
         1 input does not match format 7\n\
         1 too many operands\n\
         1 too few operands for format '%d'\n",
    );
}

#[test]
fn fscan_over_a_plain_reader_and_fscanln_over_bufio() {
    // A reader that is no io.RuneScanner is wrapped in scan.go's readRune and
    // fed one byte per Read; a *bufio.Reader is one, and is used directly, so
    // a line Fscanln leaves unread is still there for the next call.
    let src = r#"package main

import (
	"bufio"
	"fmt"
	"io"
)

type trickle struct{ data []byte }

func (t *trickle) Read(p []byte) (int, error) {
	if len(t.data) == 0 {
		return 0, io.EOF
	}
	n := copy(p, t.data[:1])
	t.data = t.data[n:]
	return n, nil
}

func main() {
	var a, b, c int
	n, err := fmt.Fscan(&trickle{[]byte("10 20\n30")}, &a, &b, &c)
	fmt.Println(n, err, a+b+c)
	br := bufio.NewReader(&trickle{[]byte("1 2\n3 4\nword\n")})
	for {
		var x, y int
		n, err := fmt.Fscanln(br, &x, &y)
		if err != nil {
			fmt.Println("stop:", n, err)
			break
		}
		fmt.Println(x * y)
	}
	var w string
	n, err = fmt.Fscanf(br, "%s\n", &w)
	fmt.Println(n, err, w)
}
"#;
    assert_stdout(
        src,
        b"",
        "3 <nil> 60\n2\n12\nstop: 0 expected integer\n1 <nil> word\n",
    );
}

#[test]
fn custom_scanner_gets_the_scan_state() {
    // A pointer implementing fmt.Scanner takes over: it reads through the
    // ScanState it is handed, by format (`Fscanf(state, …)`) or by Token.
    let src = r#"package main

import (
	"fmt"
	"strings"
)

type Point struct{ X, Y int }

func (p *Point) Scan(state fmt.ScanState, verb rune) error {
	_, err := fmt.Fscanf(state, "(%d,%d)", &p.X, &p.Y)
	return err
}

type Upper string

func (u *Upper) Scan(state fmt.ScanState, verb rune) error {
	tok, err := state.Token(true, nil)
	if err != nil {
		return err
	}
	*u = Upper(strings.ToUpper(string(tok)))
	return nil
}

func main() {
	var p Point
	var u Upper
	n, err := fmt.Sscan("(3,4) hello", &p, &u)
	fmt.Println(n, err, p, u)
	n, err = fmt.Sscan("(3;4)", &p)
	fmt.Println(n, err)
}
"#;
    assert_stdout(
        src,
        b"",
        "2 <nil> {3 4} HELLO\n0 input does not match format\n",
    );
}

#[test]
fn type_switch_tells_pointers_to_basic_types_apart() {
    // A pointer to a basic type is a cell at run time; the interface it enters
    // keeps its static type, so `*int64` is not `*int` and `*[]byte` matches.
    let src = r#"package main

import "fmt"

func set(arg any) string {
	switch v := arg.(type) {
	case *int:
		*v = 1
		return "int"
	case *int64:
		*v = 64
		return "int64"
	case *uint8:
		*v = 8
		return "uint8"
	case *string:
		*v = "s"
		return "string"
	case *[]byte:
		*v = []byte("b")
		return "[]byte"
	}
	return fmt.Sprintf("other %T", arg)
}

func main() {
	var i int
	var j int64
	var u uint8
	var s string
	var bs []byte
	p := new(int)
	fmt.Println(set(&i), set(&j), set(&u), set(&s), set(&bs), set(p), set(i))
	fmt.Println(i, j, u, s, string(bs), *p)
}
"#;
    assert_stdout(
        src,
        b"",
        "int int64 uint8 string []byte int other int\n1 64 8 s b 1\n",
    );
}

#[test]
fn embedded_interface_contributes_its_methods() {
    // `RS` embeds `RR`: a call through either destructures ReadRune's three
    // results, and a type needs both methods to satisfy RS.
    let src = r#"package main

import (
	"fmt"
	"io"
)

type RR interface {
	ReadRune() (r rune, size int, err error)
}

type RS interface {
	RR
	UnreadRune() error
}

type T struct{}

func (t *T) ReadRune() (rune, int, error) { return 'a', 1, nil }
func (t *T) UnreadRune() error            { return nil }

type OnlyRead struct{}

func (o *OnlyRead) ReadRune() (rune, int, error) { return 'b', 1, nil }

type H struct{ rs io.RuneScanner }

func main() {
	var x RS = &T{}
	a, b, c := x.ReadRune()
	fmt.Println(a, b, c)
	h := H{rs: &T{}}
	a, b, c = h.rs.ReadRune()
	fmt.Println(a, b, c)
	var any1 any = &OnlyRead{}
	_, ok := any1.(RS)
	_, ok2 := any1.(RR)
	fmt.Println(ok, ok2)
}
"#;
    assert_stdout(src, b"", "97 1 <nil>\n97 1 <nil>\nfalse true\n");
}

#[test]
fn assigning_through_a_pointer_to_an_interface_variable() {
    // `&err` of a nil `error` (here a named result, as scan.go's errorHandler
    // receives it) is the variable's own cell: `*errp = e` rebinds it.
    let src = r#"package main

import (
	"errors"
	"fmt"
	"io"
)

func handler(errp *error) {
	if e := recover(); e != nil {
		if err, ok := e.(error); ok && err == io.EOF {
			*errp = err
		}
	}
}

func run() (n int, err error) {
	defer handler(&err)
	n = 1
	panic(io.EOF)
}

func set(p *error) { *p = errors.New("set") }

func main() {
	fmt.Println(run())
	var e error
	set(&e)
	fmt.Println(e)
}
"#;
    assert_stdout(src, b"", "1 EOF\nset\n");
}

#[test]
fn array_length_from_an_imported_constant() {
    // `[utf8.UTFMax]byte` is a 4-byte array, as scan.go's readRune declares
    // its buffers — not a slice of no elements.
    let src = r#"package main

import (
	"fmt"
	"unicode/utf8"
)

type buf struct {
	b [utf8.UTFMax]byte
}

func main() {
	var x buf
	y := [utf8.RuneSelf / 32]int{}
	fmt.Println(len(x.b), len(y), x.b)
}
"#;
    assert_stdout(src, b"", "4 4 [0 0 0 0]\n");
}
