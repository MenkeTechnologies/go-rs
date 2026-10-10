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

#[test]
fn append_capacity_escape_analysis() {
    // an append to a slice that stays in its frame grows from a stack buffer
    let (out, code) = run_file(&script("append_capacity_escape_analysis"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        r#"plain 4
total 4
keep 1
ret 4
first 4
sort.Ints 4
join 2
structlit 4 1
closure 4 1
subslice 4 1
println 1 [1]
sub 4
app 4
1
pr 4
rec 4
viaClosure 4
method-leak 1
method-noleak 4 1
slices.Sort 4
slices.Contains 4 true
copyvar 4 1
copy 4 false
append-other 4 4
2d 1
4,4,4,4 9
"#
    );
}

#[test]
fn append_capacity_size_classes() {
    // growth is rounded to the allocator size class by element size
    let (out, code) = run_file(&script("append_capacity_size_classes"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        r#"int [1 2 4 8 16 32 64 128 256 512 848] 700
[]
int8 [8 16 32 64 128 256 512 896] 700
[]
int16 [4 8 16 32 64 128 256 512 896 1344 2048 3072 4096] 4000
[]
int32 [2 4 8 16 32 64 128 256 512 864] 700
[]
float64 [1 2 4 8 16 32 64 128 256 512 848] 700
[]
bool [8 16 32 64 128 256 512 896 1408 2048 3072 4096] 4000
[]
string [1 2 4 8 16 32 71 143 303 591 1023] 700
[]
P3 [1 2 4 8 16 32 64 128 256 512 853] 700
[]
PP [1 2 4 8 16 32 71 143 303 591 1023 1535 2560 3584 5120] 4000
[]
S5 [1 2 4 8 16 32 67 134 272 544 1024] 700
[]
[3]int [1 2 4 8 16 32 64 128 256 512 853] 700
[]
[]int [1 2 4 8 16 37 74 170 341 682 1135 1706 2389 3413 4778] 4000
[]
*int [1 2 4 8 16 32 64 143 287 607 1023] 700
[]
byte [8 16 32 64 128 256 512 896] 700
[]
int64 [1 2 4 8 16 32 64 128 256 512 848 1280 1792 2560 3408 5120] 4000
[]
uint16 [4 8 16 32 64 128 256 512 896] 700
[]
complex128 [1 2 4 8 16 32 64 128 256 512 848] 700
[]
any [1 2 4 8 16 32 71 143 303 591 1023 1535 2560 3584 5120] 4000
[]
map[string]int [1 2 4 8 16 32 64 143 287 607 1023] 700
[]
error [1 2 4 8 16 32 71 143 303 591 1023] 700
[]
"#
    );
}

#[test]
fn append_capacity_spread_and_multi() {
    // spread and multi-element appends allocate unless they fit the buffer
    let (out, code) = run_file(&script("append_capacity_spread_and_multi"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        r#"spread3 3
multi2 4
strspread2 8
bytes3 32
multi5 6
spread4 4
spread5 6
str3 3
slice0 4
grow-multi 8
make00 4
2d 4
"#
    );
}

#[test]
fn append_capacity_stdlib_escape() {
    // which stdlib calls keep a slice argument
    let (out, code) = run_file(&script("append_capacity_stdlib_escape"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        r#"sort.Ints 4
sort.Strings 2
sort.Float64s 4
sort.Slice 1
sort.SearchInts 4
strings.Join 2
slices.Max 4
slices.Reverse 4
slices.Clone 4
slices.Equal 4
bytes.Contains 32
errors.Join 2
fmt.Sprint(len) 4
fmt.Sprintf 1
chan 1
go-closure 1
defer-closure 4
map-store 1 1
addr 1 1
nested 4 1
iface 4
anon-struct 4 1
range 4
"#
    );
}

#[test]
fn defined_type_panic_variadic_interface_method_chain() {
    // defined types keep their type through panic and variadic interface parameters
    let (out, code) = run_file(&script(
        "defined_type_panic_variadic_interface_method_chain",
    ));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        r#"code x true code x
L3|code y|4|s|
code a
code b true code a
true false
5 5
5 15 17
true
"#
    );
}

#[test]
fn loop_variable_closure_writes_and_address() {
    // a closure that writes the loop variable, or a pointer to it, keeps its iteration
    let (out, code) = run_file(&script("loop_variable_closure_writes_and_address"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        r#"2 4 3 5 4 6 5 7 6 8 
4 5 6 7 8 
20 40 118 136 216 232 
[100 101 102 103]
a!a!! b!b!! c!c!! 
1 3 
"#
    );
}
