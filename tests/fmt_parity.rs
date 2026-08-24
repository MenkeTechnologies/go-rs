//! `fmt` verb parity with the reference `go` toolchain.
//!
//! Every expectation here is the verbatim stdout of `go run` on the same source
//! (checked against `go1.26.6 darwin/arm64`), not a guess at what Go "should"
//! print. The cases are the ones a frontend gets wrong by building the format
//! loop from the common path outward: the error forms nobody exercises until a
//! call is malformed, and the precision rule that differs between an integer and
//! a string.
//!
//! These run the built `go` binary end to end, so a regression anywhere from the
//! parser to the formatter fails a test.

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

/// Wrap `body` in a `package main` with `fmt` imported, run it, and assert its
/// stdout is exactly `expected`.
fn assert_prints(body: &str, expected: &str) {
    let src = format!("package main\n\nimport \"fmt\"\n\nfunc main() {{\n{body}\n}}\n");
    let (stdout, ok) = run(&src);
    assert!(ok, "program failed; stdout was: {stdout:?}");
    assert_eq!(stdout, expected);
}

/// A precision on an integer verb is a minimum digit count, not the truncation
/// it means for a string. Getting this backwards silently shortens numbers.
#[test]
fn integer_precision_is_a_minimum_digit_count() {
    assert_prints(
        r#"	fmt.Printf("%.3d|%.3d|%+.3d|%.3d\n", -7, 7, 7, 12345)
	fmt.Printf("%.3o|%.4b|%.4x|%.4X\n", 8, 5, 255, 255)
	fmt.Printf("%.2d|%.4x\n", uint8(7), uint64(255))"#,
        "-007|007|+007|12345\n010|0101|00ff|00FF\n07|00ff\n",
    );
}

/// Precision 0 is the one case that removes digits: a zero value prints as the
/// empty string, and any other value prints in full.
#[test]
fn precision_zero_erases_only_a_zero() {
    assert_prints(
        r#"	fmt.Printf("[%.0d][%.0d][%.d][%.0v]\n", 0, 5, 0, 0)"#,
        "[][5][][]\n",
    );
}

/// An explicit precision already fixed the digit count, so Go drops the `0` flag
/// on an integer verb and pads the rest of the width with spaces. A float keeps
/// both, because its precision counts fraction digits instead.
#[test]
fn precision_disables_zero_fill_on_an_integer_but_not_a_float() {
    assert_prints(
        r#"	fmt.Printf("[%05.2d][%-6.3d][%05d]\n", 3, 4, 42)
	fmt.Printf("[%05.1f][%08.3f]\n", 3.5, 3.14159)"#,
        "[   03][004   ][00042]\n[003.5][0003.142]\n",
    );
}

/// Octal's `#` prefix is a leading `0` — a digit — so unlike `0x` and `0b` it
/// counts toward the precision.
#[test]
fn sharp_octal_prefix_counts_toward_precision() {
    assert_prints(
        r#"	fmt.Printf("[%#.4o][%#.4x][%#.6b][%#o]\n", 8, 255, 5, 8)"#,
        "[0010][0x00ff][0b000101][010]\n",
    );
}

/// `%v` of an integer is `%d`, so it takes the digit-count precision and the
/// zero fill. `+` and `#` are not number flags and must not divert it.
#[test]
fn percent_v_of_an_integer_formats_as_percent_d() {
    assert_prints(
        r#"	fmt.Printf("[%.2v][%05v][%5v][%-5v]\n", -3, 42, 42, 42)
	fmt.Printf("[%+v][%#v][%05v]\n", 42, 42, "ab")"#,
        "[-03][00042][   42][42   ]\n[42][42][000ab]\n",
    );
}

/// The space flag leaves room for the sign a non-negative number elides, and `+`
/// outranks it. On a string or byte slice in hex it does something else
/// entirely: it separates the bytes.
#[test]
fn space_flag_stands_in_for_an_elided_sign() {
    assert_prints(
        r#"	fmt.Printf("[% d][% d][%+ d][% 5d][% 05d]\n", 42, -42, 42, 42, 42)
	fmt.Printf("[% f][% f][% e][% v]\n", 1.5, -1.5, 1.5, 42)
	fmt.Printf("[% x][% X][% x][% o]\n", 255, 255, -255, 8)
	fmt.Printf("[% x][% x][% d]\n", "abc", []byte("ab"), []int{1, -2})"#,
        "[ 42][-42][+42][   42][ 0042]\n\
         [ 1.500000][-1.500000][ 1.500000e+00][ 42]\n\
         [ ff][ FF][-ff][ 10]\n\
         [61 62 63][61 62][[ 1 -2]]\n",
    );
}

/// Too few operands, too many, and a verb that is not a verb. A formatter that
/// substitutes a zero value for a missing operand reports a malformed call as a
/// well-formed one, which is the failure this pins.
#[test]
fn missing_extra_and_unknown_verbs_report_themselves() {
    assert_prints(
        r#"	fmt.Println(fmt.Sprintf("%d %s", 1))
	fmt.Println(fmt.Sprintf("%d %d %d", 1))
	fmt.Println(fmt.Sprintf("%s"))
	fmt.Println(fmt.Sprintf("%T"))
	fmt.Println(fmt.Sprintf("%d", 1, 2))
	fmt.Println(fmt.Sprintf("%d", 1, 2, "x"))
	fmt.Println(fmt.Sprintf("no verbs", 1))
	fmt.Println(fmt.Sprintf("%z", 1))
	fmt.Println(fmt.Sprintf("%y %d", 1, 2))
	fmt.Println(fmt.Sprintf("%5.2z", 3))"#,
        "1 %!s(MISSING)\n\
         1 %!d(MISSING) %!d(MISSING)\n\
         %!s(MISSING)\n\
         %!T(MISSING)\n\
         1%!(EXTRA int=2)\n\
         1%!(EXTRA int=2, string=x)\n\
         no verbs%!(EXTRA int=1)\n\
         %!z(int=1)\n\
         %!y(int=1) 2\n\
         %!z(int=   03)\n",
    );
}

/// A `%` the format never carries to a verb character. `%%` is the one form that
/// consumes no operand, so an argument beside it is still extra.
#[test]
fn a_percent_with_no_verb_is_noverb() {
    assert_prints(
        r#"	fmt.Println(fmt.Sprintf("%"))
	fmt.Println(fmt.Sprintf("abc%"))
	fmt.Println(fmt.Sprintf("%d%", 1))
	fmt.Println(fmt.Sprintf("%!"))
	fmt.Println(fmt.Sprintf("%%d", 1))"#,
        "%!(NOVERB)\n\
         abc%!(NOVERB)\n\
         1%!(NOVERB)\n\
         %!!(MISSING)\n\
         %d%!(EXTRA int=1)\n",
    );
}

/// `*` takes the width or precision from an operand — including the negative
/// width that means left-alignment — and reports a non-`int` there without
/// losing the verb.
#[test]
fn star_width_and_precision_read_an_operand() {
    assert_prints(
        r#"	fmt.Println(fmt.Sprintf("[%*d][%-*d][%*d][%0*d][%*s]", 6, 42, 6, 42, -6, 42, 5, 42, 5, "ab"))
	fmt.Println(fmt.Sprintf("[%.*f][%*.*f]", 2, 3.14159, 9, 2, 3.14159))
	fmt.Println(fmt.Sprintf("[%*d]", 6, 2))
	fmt.Println(fmt.Sprintf("[%.*f]", 2))
	fmt.Println(fmt.Sprintf("[%*d]", "x", 42))
	fmt.Println(fmt.Sprintf("[%.*f]", "x", 1.5))"#,
        "[    42][42    ][42    ][00042][   ab]\n\
         [3.14][     3.14]\n\
         [     2]\n\
         [%!f(MISSING)]\n\
         [%!(BADWIDTH)42]\n\
         [%!(BADPREC)1.500000]\n",
    );
}

/// `%T` of a sized integer. All of these are one `Value::Int` at run time, so
/// the width lives only in the static type — drop the tag and every line answers
/// `int`.
#[test]
fn percent_t_names_every_sized_integer_width() {
    assert_prints(
        r#"	var i8 int8 = 5
	var i16 int16 = 5
	var i32 int32 = 5
	var i64 int64 = 5
	var u8 uint8 = 5
	var u16 uint16 = 5
	var u32 uint32 = 5
	fmt.Printf("%T %T %T %T\n", i8, i16, i32, i64)
	fmt.Printf("%T %T %T\n", u8, u16, u32)
	fmt.Printf("%T %T %T\n", byte(1), rune(1), int8(1)+int8(2))
	xs := []int8{1, 2}
	m := map[string]uint8{"a": 1}
	fmt.Printf("%T %T %T %T\n", xs, xs[0], m, m["a"])"#,
        "int8 int16 int32 int64\n\
         uint8 uint16 uint32\n\
         uint8 int32 int8\n\
         []int8 int8 map[string]uint8 uint8\n",
    );
}

/// A defined type is named, not described — it outranks the width of the base it
/// is represented as.
#[test]
fn a_defined_type_outranks_its_base_width() {
    let src = r#"package main

import "fmt"

type myByte byte
type myCount int32

func main() {
	var mb myByte = 7
	var mc myCount = 7
	fmt.Printf("%T %T %v %d\n", mb, mc, mb, mc)
	fmt.Printf("%T\n", map[myByte]myCount{1: 2})
}
"#;
    let (stdout, ok) = run(src);
    assert!(ok, "program failed; stdout was: {stdout:?}");
    assert_eq!(
        stdout,
        "main.myByte main.myCount 7 7\nmap[main.myByte]main.myCount\n"
    );
}

/// `f(args...)` into a `fmt` call spreads into the operand list. This is the
/// shape every logging wrapper in Go is written in; passing the slice whole
/// prints `[1 2]` where Go prints `1-2`, and the call still "succeeds".
#[test]
fn a_spread_reaches_fmt_as_separate_operands() {
    let src = r#"package main

import "fmt"

func logf(f string, a ...any) { fmt.Printf(f, a...) }

func joined(a ...any) string { return fmt.Sprint(a...) }

func wrapped(f string, a ...any) error { return fmt.Errorf("w: "+f, a...) }

func main() {
	logf("%d-%d\n", 1, 2)
	logf("none\n")
	fmt.Println(joined(1, 2, "x"))
	fmt.Println(joined())
	fmt.Println(wrapped("%d", 9))
	var none []any
	fmt.Printf("[%s]\n", fmt.Sprint(none...))
}
"#;
    let (stdout, ok) = run(src);
    assert!(ok, "program failed; stdout was: {stdout:?}");
    assert_eq!(stdout, "1-2\nnone\n1 2x\n\nw: 9\n[]\n");
}

/// `%w` is `fmt.Errorf`'s wrap verb and renders as `%v`. The unknown-verb path
/// must not swallow it, or every wrapped error's message becomes `%!w(…)`.
#[test]
fn errorf_wrap_verb_renders_the_wrapped_error() {
    let src = r#"package main

import (
	"errors"
	"fmt"
)

func main() {
	base := errors.New("gone")
	e := fmt.Errorf("step: %w", base)
	fmt.Println(e)
	fmt.Println(errors.Is(e, base), errors.Unwrap(e) == base)
}
"#;
    let (stdout, ok) = run(src);
    assert!(ok, "program failed; stdout was: {stdout:?}");
    assert_eq!(stdout, "step: gone\ntrue true\n");
}

/// An explicit `[n]` operand index reaches the nth operand and re-reads it. Go
/// tries for one in three places — before the width, before the precision, and
/// before the verb — so a `*` width can be indexed independently of the value it
/// sizes. Every expectation is `go run`'s verbatim stdout.
#[test]
fn an_explicit_argument_index_selects_the_operand() {
    assert_prints(
        r#"	fmt.Printf("[%[1]d][%[2]s][%[1]v]\n", 3, "b")
	fmt.Printf("[%[2]d %[1]d %[1]s]\n", "x", 7)
	fmt.Printf("[%[1]*d][%[2]*[1]d]\n", 3, 4)
	fmt.Printf("[%.[2]d]\n", 5, 3)"#,
        "[3][b][3]\n[7 %!d(string=x) x]\n[  4][   3]\n[3]\n",
    );
}

/// A `[n]` that names no operand, or does not parse, is `%!verb(BADINDEX)` —
/// and a width or precision written *after* the bracket is the same error,
/// because the index has to sit immediately before what it selects.
#[test]
fn a_malformed_argument_index_is_badindex() {
    assert_prints(
        r#"	fmt.Printf("[%[3]d][%[0]d]\n", 1, 2)
	fmt.Printf("[%[x]d][%[]d]\n", 1)
	fmt.Printf("[%[1]2d][%[1].2d]\n", 5)"#,
        "[%!d(BADINDEX)][%!d(BADINDEX)]\n[%!d(BADINDEX)][%!d(BADINDEX)]\n[%!d(BADINDEX)][%!d(BADINDEX)]\n",
    );
}

/// Once a format has reordered its operands, Go stops reporting the ones it
/// never reached: with the cursor moved about, a trailing operand is no evidence
/// of an unused argument. A format that never indexed still reports them.
#[test]
fn an_index_suppresses_the_extra_report() {
    assert_prints(
        r#"	fmt.Printf("[%[1]d]\n", 1, 2, 3)
	fmt.Printf("[%d]\n", 1, 2)"#,
        "[1]\n[1]\n%!(EXTRA int=2)",
    );
}

/// Go's `writePadding` fills with `0` for every verb the flag reaches, not just
/// the numeric ones — `%010q` of a string is zero-filled and so is `%010T`.
/// `%U` is the one exception: `fmt.fmtUnicode` clears the flag itself.
#[test]
fn the_zero_flag_fills_every_verb_but_unicode() {
    assert_prints(
        r#"	fmt.Printf("[%010s][%010q][%010t][%010c]\n", "z", "z", true, 'A')
	fmt.Printf("[%010U][%010T][%08.3g]\n", 'A', 5, 1.5)
	fmt.Printf("[%05.2s][%05.2q]\n", "abcdef", "abcdef")"#,
        "[000000000z][0000000\"z\"][000000true][000000000A]\n[    U+0041][0000000int][000001.5]\n[000ab][0\"ab\"]\n",
    );
}

/// A composite under `%v` takes the width at each *element*, the way Go's
/// `printValue` walks it — padding the list as a whole is the shape a frontend
/// reaches for first and it is wrong at every depth.
#[test]
fn a_width_on_percent_v_applies_to_each_element() {
    let src = r#"package main

import "fmt"

type S struct {
	A int
	B string
}

func main() {
	fmt.Printf("[%10v]\n", []int{1, 2})
	fmt.Printf("[%010v]\n", []int{1, 2})
	fmt.Printf("[%10v]\n", S{1, "x"})
	fmt.Printf("[%010v]\n", map[string]int{"a": 1})
	fmt.Printf("[%010s]\n", []string{"a"})
	var nilS []int
	fmt.Printf("[%010v]\n", nilS)
}
"#;
    let (stdout, ok) = run(src);
    assert!(ok, "program failed; stdout was: {stdout:?}");
    assert_eq!(
        stdout,
        "[[         1          2]]\n[[0000000001 0000000002]]\n[{         1          x}]\n\
         [map[000000000a:0000000001]]\n[[000000000a]]\n[[]]\n"
    );
}

/// A nil operand under an unknown verb has no type to name, so Go writes the
/// bare `%!y(<nil>)` rather than the `type=value` form a typed operand gets.
#[test]
fn an_unknown_verb_on_nil_names_no_type() {
    assert_prints(
        r#"	fmt.Printf("[%p][%y][%y]\n", nil, nil, 5)"#,
        "[%!p(<nil>)][%!y(<nil>)][%!y(int=5)]\n",
    );
}
