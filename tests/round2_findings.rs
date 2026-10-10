//! Regressions for the second round of differential fuzzing against the
//! reference `go`. Each program is the `parity-scripts/` file that reproduces
//! it; the expected text is the verbatim stdout of `go run` on that file, so
//! the tests need no `go` toolchain.

use std::process::Command;

/// Run a program file through the built `go` binary; return (stdout, exit code).
fn run_file(path: &str) -> (String, i32) {
    let out = Command::new(env!("CARGO_BIN_EXE_go"))
        .arg("run")
        .arg(path)
        .output()
        .expect("spawn go binary");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

fn script(name: &str) -> String {
    format!("{}/parity-scripts/{name}.go", env!("CARGO_MANIFEST_DIR"))
}

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

#[test]
fn closed_channel_misuse() {
    // close of a closed or nil channel and a send on a closed one are recoverable runtime errors
    let (out, code) = run_file(&script("closed_channel_misuse"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        r#"close closed close of closed channel true close of closed channel
send closed send on closed channel true send on closed channel
select send closed send on closed channel true send on closed channel
close nil close of nil channel true close of nil channel
0 false
7 true
0 false
goroutine close of closed channel
"#
    );
}

#[test]
fn shift_count_past_width() {
    // a shift count at or past the width shifts everything out
    let (out, code) = run_file(&script("shift_count_past_width"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        r#"true 0 -1 -1
0 0 2147483648
0 0 0 15 -1 -1
0 -1 792
0 -1 0
1152921504606846976 1152921504606846976 -1|4611686018427387904 4611686018427387904 -1|0 0 -1|0 0 -1|
32 4 32 1.1805916207174113e+21
6 3
6
"#
    );
}

#[test]
fn integer_wraparound_by_width() {
    // fixed-width integer arithmetic wraps at the type width
    let (out, code) = run_file(&script("integer_wraparound_by_width"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        r#"-128
255
32767 -32767
1
-2
0
-9223372036854775808
0
18446744073709551614
true 0
-128 0 -128
44 144 144 100 55
-3 96 251 -5 4294967291 18446744073709551611
-559038737 -16657 -17 48879
true 0 -1 -1
-4 -1 -5 15
127 -128 4294967295
254 1 3 127 1
18446744073709551615 1
128 0 0
true
1114208
132 
3 -3 3
65535 1 4294967295
-128 -1 377 -11 FFFF
00000101 0xff 010 +5  5
"#
    );
}

#[test]
fn pointer_to_scalars() {
    // a pointer to a scalar variable, field or element reads and writes through
    let (out, code) = run_file(&script("pointer_to_scalars"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        r#"8 8
[1 20 4] 20
2 1
8
11
100
true false true
true
{8 5 xyz}
4
11 11
[9 0 0]
1 3 5 
"#
    );
}

#[test]
fn sync_atomic() {
    // sync/atomic functions and types
    let (out, code) = run_file(&script("sync_atomic"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        r#"5 5
3 true 9
15
"#
    );
}

#[test]
fn time_duration_and_calendar() {
    // the deterministic subset of the time package
    let (out, code) = run_file(&script("time_duration_and_calendar"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        r#"1h30m30.5s 1.5084722222222222 90.50833333333334 5430.5 5430500
1.5ms 0s 2ns 1.5s -3s
1h0m0s 2h0m0s 999ns
1s 1000000000 1ms time.Duration
2024-02-29 13:04:05.123456789 +0000 UTC
2024 February 29 13 4 5 123456789 Thursday 60
2024-02-29T13:04:05Z 2024-02-29T13:04:05.123456789Z Thu, 29 Feb 2024 13:04:05 UTC 1:04PM
Thu Feb 29 13:04:05 2024 29/02/24 01:04PM 2024-02-29T13:04:05.123Z
1709211845 1709211845123456789 1709211845123
2024-03-02 01:04:05.123456789 +0000 UTC 36h0m0s true true true
2024-03-30 2025-03-01
1970-01-01 00:00:00 +0000 UTC 2023-11-14 22:13:20
true 0001-01-01 00:00:00 +0000 UTC -62135596800
2023-07-04 09:08:07 +0000 UTC <nil>
parsing time "2023-13-04": month out of range
parsing time "hello" as "2006-01-02": cannot parse "hello" as "2006"
1h15m30.5s <nil>
time: invalid duration "abc"
13:00:00 13:00:00
March Saturday %!Month(13)
2024-02-29 08:04:05.123456789 -0500 EST 29 Feb 24 08:04 -0500
true true
fast slow
timeout
ticks 3
true false
true
"#
    );
}

#[test]
fn generic_type_parameter_conversion() {
    // a type parameter converts, zeroes and sizes by its resolved type argument
    let (out, code) = run_file(&script("generic_type_parameter_conversion"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        r#"2.3333333333333335 3 1.5
1.5 6
0 true 0 false true
"" 0 0 7
3 3
[3 4] [3 6]
3 4
["" ""] [0 0]
[2 4]
"#
    );
}

#[test]
fn slice_to_array_and_fmt_operand_state() {
    // slice to array conversion and fmt operands printed after all arguments are evaluated
    let (out, code) = run_file(&script("slice_to_array_and_fmt_operand_state"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        r#"[7 8] [100 8 9] true
[{1 2} {3 4}] [{50 2} {3 4}]
[2]int 4
runtime error: cannot convert slice with length 1 to array or pointer to array with length 2
[1 1 2] 2
map[a:1 b:2] 2
xyz 3
[-1 1 2] 0
"#
    );
}

#[test]
fn sharp_v_typed_nil_fields() {
    // percent-sharp-v names the type of a nil pointer, func, chan or interface
    let (out, code) = run_file(&script("sharp_v_typed_nil_fields"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        r#"main.Holder{P:(*int)(nil), F:(func())(nil), G:(func(int) string)(nil), E:error(nil), C:(chan int)(nil), I:interface {}(nil), S:main.Shape(nil), N:(*main.Node)(nil), M:map[string]int(nil), L:[]int(nil), ok:false}
(*int)(nil) (*main.Node)(nil) (func())(nil) (chan string)(nil)
<nil> <nil> true <nil>
*int *main.Node chan string
5 2 true
main.Node{Next:(*main.Node)(nil), Val:0}
"#
    );
}

#[test]
fn nil_dereference_panics() {
    // faults on nil values are recoverable runtime errors with Go text
    let (out, code) = run_file(&script("nil_dereference_panics"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        r#"field runtime error: invalid memory address or nil pointer dereference
assign runtime error: invalid memory address or nil pointer dereference
method runtime error: invalid memory address or nil pointer dereference
valmethod runtime error: invalid memory address or nil pointer dereference
deref runtime error: invalid memory address or nil pointer dereference
intderef runtime error: invalid memory address or nil pointer dereference
intassign runtime error: invalid memory address or nil pointer dereference
nilmap assignment to entry in nil map
idx runtime error: index out of range [0] with length 0
nilfunc runtime error: invalid memory address or nil pointer dereference
nilerr runtime error: invalid memory address or nil pointer dereference
assert interface conversion: interface {} is nil, not int
slice runtime error: slice bounds out of range [:5] with capacity 0
div runtime error: integer divide by zero
custom x5
nilpanic runtime error: panic called with nil argument
"#
    );
}

#[test]
fn exact_constant_arithmetic() {
    // untyped constant arithmetic is exact
    let (out, code) = run_file(&script("exact_constant_arithmetic"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        r#"1 2 4 30 40 60
4 2 2.5 98 xy 100 1000 194 1 -3 -1 240 5 -1 255 8
1024 1.048576e+06 1.073741824e+09
R G B
B 2 float64 float64 float64
0 0 1 2 2 4
255
2 2.5
4 1.2676506002282294e+30
int 8
6 3 2
6
3.5 3 3.5 0.3333333333333333
0.3 0.3
97 98 b
"#
    );
}

#[test]
fn unrecovered_close_of_closed_channel_exits_2_with_go_text() {
    let (out, err, code) = run_src(
        "package main\n\nimport \"fmt\"\n\nfunc main() {\n\tch := make(chan int)\n\tclose(ch)\n\tfmt.Println(\"before\")\n\tclose(ch)\n\tfmt.Println(\"after\")\n}\n",
    );
    assert_eq!(out, "before\n");
    assert_eq!(code, 2);
    assert!(err.starts_with("panic: close of closed channel"), "{err}");
}

#[test]
fn unrecovered_send_on_closed_channel_runs_defers_then_exits_2() {
    let (out, err, code) = run_src(
        "package main\n\nimport \"fmt\"\n\nfunc main() {\n\tdefer fmt.Println(\"deferred\")\n\tch := make(chan int, 1)\n\tclose(ch)\n\tch <- 1\n}\n",
    );
    assert_eq!(out, "deferred\n");
    assert_eq!(code, 2);
    assert!(err.starts_with("panic: send on closed channel"), "{err}");
}

#[test]
fn unrecovered_nil_pointer_store_exits_2() {
    let (out, err, code) = run_src(
        "package main\n\nimport \"fmt\"\n\nfunc main() {\n\tvar p *int\n\tfmt.Println(\"x\")\n\t*p = 1\n}\n",
    );
    assert_eq!(out, "x\n");
    assert_eq!(code, 2);
    assert!(
        err.starts_with("panic: runtime error: invalid memory address or nil pointer dereference"),
        "{err}"
    );
}

#[test]
fn pointers_to_distinct_scalars_and_to_one_field_compare_by_address() {
    let (out, _, code) = run_src(
        "package main\n\nimport \"fmt\"\n\ntype S struct{ a, b int }\n\nfunc main() {\n\tx, y := 1, 1\n\tpx, py := &x, &y\n\tfmt.Println(px == py, px == &x, &x == &x)\n\ts := S{}\n\tfmt.Println(&s.a == &s.a, &s.a == &s.b)\n\txs := []int{1, 2}\n\tys := xs[:]\n\tfmt.Println(&xs[0] == &ys[0], &xs[0] == &xs[1])\n\t*px = 9\n\tfmt.Println(x, y)\n}\n",
    );
    assert_eq!(code, 0);
    assert_eq!(out, "false true true\ntrue false\ntrue false\n9 1\n");
}

#[test]
fn pointer_to_call_results() {
    // a variable bound from a scalar-result call is addressable
    let (out, code) = run_file(&script("pointer_to_call_results"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        r#"9 <nil>
11 5
7 named
3
y x
2.5
20 20
"#
    );
}
