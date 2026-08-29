//! End-to-end tests: run a `.go` program through the actual `go` binary and
//! assert its stdout. These exercise the whole pipeline — lexer (with automatic
//! semicolon insertion) → parser → compiler → fusevm execution — exactly as a
//! user invokes it, so a regression anywhere in the chain fails a test.

use std::io::Write;
use std::process::Command;

/// Compile and run `src` through the built `go` binary; return (stdout, success).
fn run(src: &str) -> (String, bool) {
    let mut f = tempfile::Builder::new()
        .suffix(".go")
        .tempfile()
        .expect("temp file");
    f.write_all(src.as_bytes()).expect("write source");
    let path = f.path().to_owned();

    let out = Command::new(env!("CARGO_BIN_EXE_go"))
        .arg("run")
        .arg(&path)
        .output()
        .expect("spawn go binary");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.success(),
    )
}

/// As [`run`], but returns stderr as well — for programs expected to be
/// rejected, where the diagnostic is the thing under test.
fn run_capturing_stderr(src: &str) -> (String, bool) {
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
        String::from_utf8_lossy(&out.stderr).into_owned() + &String::from_utf8_lossy(&out.stdout),
        out.status.success(),
    )
}

/// Assert a program runs successfully and prints exactly `expected` on stdout.
fn assert_stdout(src: &str, expected: &str) {
    let (stdout, ok) = run(src);
    assert!(ok, "program failed; stdout was: {stdout:?}");
    assert_eq!(stdout, expected);
}

#[test]
fn hello_world() {
    assert_stdout(
        "package main\nimport \"fmt\"\nfunc main() {\n\tfmt.Println(\"hello, world\")\n}\n",
        "hello, world\n",
    );
}

#[test]
fn integer_arithmetic_and_precedence() {
    // 2 + 3*4 == 14, printed by fmt.Println.
    assert_stdout(
        "package main\nimport \"fmt\"\nfunc main() {\n\tfmt.Println(2 + 3*4)\n}\n",
        "14\n",
    );
}

#[test]
fn integer_division_truncates() {
    // Go truncates int/int toward zero: 7/2 == 3.
    assert_stdout(
        "package main\nimport \"fmt\"\nfunc main() {\n\tfmt.Println(7 / 2)\n}\n",
        "3\n",
    );
}

#[test]
fn float_division_is_exact() {
    assert_stdout(
        "package main\nimport \"fmt\"\nfunc main() {\n\tfmt.Println(7.0 / 2.0)\n}\n",
        "3.5\n",
    );
}

#[test]
fn whole_float_prints_without_fraction() {
    // Go's %v prints 3.0 as `3`.
    assert_stdout(
        "package main\nimport \"fmt\"\nfunc main() {\n\tfmt.Println(6.0 / 2.0)\n}\n",
        "3\n",
    );
}

#[test]
fn string_concatenation() {
    assert_stdout(
        "package main\nimport \"fmt\"\nfunc main() {\n\tfmt.Println(\"a\" + \"b\" + \"c\")\n}\n",
        "abc\n",
    );
}

#[test]
fn booleans_and_comparisons() {
    assert_stdout(
        "package main\nimport \"fmt\"\nfunc main() {\n\tfmt.Println(3 < 5, 5 <= 5, 2 == 3)\n}\n",
        "true true false\n",
    );
}

#[test]
fn recursion_fibonacci() {
    let src = "\
package main
import \"fmt\"
func fib(n int) int {
	if n < 2 {
		return n
	}
	return fib(n-1) + fib(n-2)
}
func main() {
	fmt.Println(fib(10))
}
";
    assert_stdout(src, "55\n");
}

#[test]
fn accumulating_loop() {
    let src = "\
package main
import \"fmt\"
func main() {
	sum := 0
	for i := 1; i <= 100; i++ {
		sum += i
	}
	fmt.Println(sum)
}
";
    assert_stdout(src, "5050\n");
}

#[test]
fn break_and_continue() {
    // Count odd numbers below 10, stopping at 7: 1, 3, 5 -> 3.
    let src = "\
package main
import \"fmt\"
func main() {
	c := 0
	for i := 0; i < 10; i++ {
		if i == 7 {
			break
		}
		if i%2 == 0 {
			continue
		}
		c++
	}
	fmt.Println(c)
}
";
    assert_stdout(src, "3\n");
}

#[test]
fn printf_verbs() {
    let src = "\
package main
import \"fmt\"
func main() {
	fmt.Printf(\"%d and %s and %t\\n\", 42, \"hi\", true)
}
";
    assert_stdout(src, "42 and hi and true\n");
}

#[test]
fn fizzbuzz_first_five() {
    let src = "\
package main
import \"fmt\"
func main() {
	for i := 1; i <= 5; i++ {
		if i%15 == 0 {
			fmt.Println(\"FizzBuzz\")
		} else if i%3 == 0 {
			fmt.Println(\"Fizz\")
		} else if i%5 == 0 {
			fmt.Println(\"Buzz\")
		} else {
			fmt.Println(i)
		}
	}
}
";
    assert_stdout(src, "1\n2\nFizz\n4\nBuzz\n");
}

#[test]
fn undefined_function_is_a_compile_error() {
    let (_stdout, ok) = run("package main\nfunc main() {\n\tnope()\n}\n");
    assert!(!ok, "calling an undefined function should fail");
}

// ── composite types: slices, maps, structs, methods, range, stdlib ──────────

#[test]
fn slice_literal_index_len_append() {
    let src = "\
package main
import \"fmt\"
func main() {
	xs := []int{3, 1, 2}
	xs = append(xs, 4)
	xs[0] = 9
	fmt.Println(xs, len(xs), xs[3])
}
";
    assert_stdout(src, "[9 1 2 4] 4 4\n");
}

#[test]
fn make_slice_zero_filled() {
    let src = "\
package main
import \"fmt\"
func main() {
	ys := make([]int, 3)
	ys[1] = 5
	fmt.Println(ys)
}
";
    assert_stdout(src, "[0 5 0]\n");
}

#[test]
fn range_over_slice_sums() {
    let src = "\
package main
import \"fmt\"
func main() {
	xs := []int{10, 20, 30}
	sum := 0
	for i, v := range xs {
		sum += i + v
	}
	fmt.Println(sum)
}
";
    // (0+10)+(1+20)+(2+30) = 63
    assert_stdout(src, "63\n");
}

#[test]
fn map_literal_index_delete() {
    let src = "\
package main
import \"fmt\"
func main() {
	m := map[string]int{\"a\": 1, \"b\": 2}
	m[\"c\"] = 3
	delete(m, \"a\")
	fmt.Println(m, len(m), m[\"b\"])
}
";
    // fmt sorts map keys.
    assert_stdout(src, "map[b:2 c:3] 2 2\n");
}

#[test]
fn range_over_map_sums_values() {
    let src = "\
package main
import \"fmt\"
func main() {
	m := map[string]int{\"a\": 1, \"b\": 2, \"c\": 3}
	sum := 0
	for _, v := range m {
		sum += v
	}
	fmt.Println(sum)
}
";
    assert_stdout(src, "6\n");
}

#[test]
fn struct_value_semantics_and_methods() {
    let src = "\
package main
import \"fmt\"
type Point struct {
	x int
	y int
}
func (p Point) sum() int {
	return p.x + p.y
}
func main() {
	p := Point{x: 3, y: 4}
	q := p
	q.x = 100
	fmt.Println(p, q, p.sum())
}
";
    // q is a copy — mutating q.x must not change p.
    assert_stdout(src, "{3 4} {100 4} 7\n");
}

#[test]
fn struct_positional_literal_and_field_update() {
    let src = "\
package main
import \"fmt\"
type Counter struct {
	n int
}
func main() {
	c := Counter{0}
	c.n += 5
	c.n++
	fmt.Println(c.n)
}
";
    assert_stdout(src, "6\n");
}

#[test]
fn strings_stdlib() {
    let src = "\
package main
import (
	\"fmt\"
	\"strings\"
)
func main() {
	fmt.Println(strings.ToUpper(\"go\"), strings.Contains(\"golang\", \"lang\"))
	parts := strings.Split(\"a,b,c\", \",\")
	fmt.Println(strings.Join(parts, \"-\"), len(parts))
}
";
    assert_stdout(src, "GO true\na-b-c 3\n");
}

#[test]
fn strconv_stdlib() {
    // `strconv.Atoi` returns Go's `(int, error)` pair, so the value is bound by
    // destructuring. (This test previously asserted `strconv.Atoi("100")+1` —
    // a single-value use of a two-value call, which the real `go` toolchain
    // rejects with "multiple-value strconv.Atoi(...) in single-value context".
    // go-rs now rejects it too; see `multi_value_call_in_single_value_context`.)
    let src = "\
package main
import (
	\"fmt\"
	\"strconv\"
)
func main() {
	n, err := strconv.Atoi(\"100\")
	fmt.Println(strconv.Itoa(42), n+1, err)
	_, err = strconv.Atoi(\"xx\")
	fmt.Println(err)
	f, ferr := strconv.ParseFloat(\"2.5\", 64)
	fmt.Println(f, ferr)
	h, herr := strconv.ParseInt(\"ff\", 16, 64)
	fmt.Println(h, herr)
}
";
    assert_stdout(
        src,
        "42 101 <nil>\nstrconv.Atoi: parsing \"xx\": invalid syntax\n2.5 <nil>\n255 <nil>\n",
    );
}

#[test]
fn multi_value_call_in_single_value_context() {
    // Go rejects using a two-value call where one value is expected; go-rs's
    // tuple is an ordinary slice value, so without the check the operand would
    // silently be that slice.
    let (_stdout, ok) = run("\
package main
import (
	\"fmt\"
	\"strconv\"
)
func main() {
	fmt.Println(strconv.Atoi(\"100\") + 1)
}
");
    assert!(
        !ok,
        "a two-value call in single-value context must not compile"
    );

    let (_stdout, ok) = run("\
package main
func two() (int, int) { return 1, 2 }
func main() {
	n := two()
	_ = n
}
");
    assert!(!ok, "binding a two-value call to one name must not compile");
}

#[test]
fn slice_index_out_of_range_errors() {
    let (_stdout, ok) = run("package main\nfunc main() {\n\txs := []int{1}\n\t_ = xs[5]\n}\n");
    assert!(!ok, "out-of-range slice index should fail at runtime");
}

#[test]
fn select_picks_ready_channel() {
    let src = "\
package main
import \"fmt\"
func main() {
	ch1 := make(chan int, 1)
	ch2 := make(chan int, 1)
	ch2 <- 7
	select {
	case v := <-ch1:
		fmt.Println(\"ch1\", v)
	case v := <-ch2:
		fmt.Println(\"ch2\", v)
	}
}
";
    assert_stdout(src, "ch2 7\n");
}

#[test]
fn select_default_when_nothing_ready() {
    let src = "\
package main
import \"fmt\"
func main() {
	ch := make(chan int)
	select {
	case v := <-ch:
		fmt.Println(v)
	default:
		fmt.Println(\"none\")
	}
}
";
    assert_stdout(src, "none\n");
}

#[test]
fn select_blocks_until_a_goroutine_sends() {
    let src = "\
package main
import \"fmt\"
func main() {
	done := make(chan int)
	go func() {
		done <- 99
	}()
	select {
	case v := <-done:
		fmt.Println(v)
	}
}
";
    assert_stdout(src, "99\n");
}

#[test]
fn closure_captures_local_by_value() {
    let src = "\
package main
import \"fmt\"
func main() {
	factor := 3
	triple := func(x int) int {
		return x * factor
	}
	fmt.Println(triple(5), triple(10))
}
";
    assert_stdout(src, "15 30\n");
}

#[test]
fn immediately_invoked_function_literal() {
    let src = "\
package main
import \"fmt\"
func main() {
	fmt.Println(func(a int, b int) int { return a + b }(10, 20))
}
";
    assert_stdout(src, "30\n");
}

#[test]
fn goroutine_closure_captures_channel() {
    let src = "\
package main
import \"fmt\"
func main() {
	done := make(chan int)
	msg := 42
	go func() {
		done <- msg
	}()
	fmt.Println(<-done)
}
";
    assert_stdout(src, "42\n");
}

#[test]
fn interface_dynamic_dispatch() {
    let src = "\
package main
import \"fmt\"
type Shape interface {
	area() int
}
type Rect struct {
	w int
	h int
}
func (r Rect) area() int {
	return r.w * r.h
}
type Square struct {
	s int
}
func (sq Square) area() int {
	return sq.s * sq.s
}
func describe(s Shape) {
	fmt.Println(s.area())
}
func main() {
	describe(Rect{w: 3, h: 4})
	describe(Square{s: 5})
}
";
    // Dispatch to the concrete type behind the interface at runtime.
    assert_stdout(src, "12\n25\n");
}

#[test]
fn goroutine_unbuffered_channel_handshake() {
    let src = "\
package main
import \"fmt\"
func send(ch chan int) {
	ch <- 42
}
func main() {
	ch := make(chan int)
	go send(ch)
	fmt.Println(<-ch)
}
";
    assert_stdout(src, "42\n");
}

#[test]
fn goroutine_worker_over_buffered_channels() {
    let src = "\
package main
import \"fmt\"
func worker(jobs chan int, results chan int) {
	for {
		j := <-jobs
		results <- j * 2
	}
}
func main() {
	jobs := make(chan int, 5)
	results := make(chan int, 5)
	go worker(jobs, results)
	for i := 1; i <= 5; i++ {
		jobs <- i
	}
	sum := 0
	for i := 0; i < 5; i++ {
		sum += <-results
	}
	fmt.Println(sum)
}
";
    // (1+2+3+4+5)*2 = 30
    assert_stdout(src, "30\n");
}

#[test]
fn goroutine_deadlock_is_reported() {
    // main receives on a channel nothing sends to.
    let (_stdout, ok) = run("package main\nfunc main() {\n\tch := make(chan int)\n\t_ = <-ch\n}\n");
    assert!(!ok, "a receive with no sender should deadlock and fail");
}

#[test]
fn interface_slice_polymorphism() {
    let src = "\
package main
import \"fmt\"
type Stringer interface {
	label() string
}
type A struct{}
func (a A) label() string {
	return \"a\"
}
type B struct{}
func (b B) label() string {
	return \"b\"
}
func main() {
	xs := []Stringer{A{}, B{}, A{}}
	out := \"\"
	for _, x := range xs {
		out += x.label()
	}
	fmt.Println(out)
}
";
    assert_stdout(src, "aba\n");
}

// ── regressions found by the parity harness ─────────────────────────────────

#[test]
fn printf_width_and_precision() {
    let src = "\
package main
import \"fmt\"
func main() {
	fmt.Printf(\"%.4f|%8.2f|%-8.2f|%05d|%x|%-6s|end\\n\", 3.14159, 1.5, 1.5, 42, 255, \"hi\")
}
";
    assert_stdout(src, "3.1416|    1.50|1.50    |00042|ff|hi    |end\n");
}

#[test]
fn elided_struct_literals_in_slice() {
    let src = "\
package main
import \"fmt\"
type P struct {
	x int
	y int
}
func main() {
	ps := []P{{x: 1, y: 2}, {x: 3, y: 4}}
	sum := 0
	for _, p := range ps {
		sum += p.x + p.y
	}
	fmt.Println(sum, ps[1])
}
";
    assert_stdout(src, "10 {3 4}\n");
}

#[test]
fn multi_value_assign_from_call() {
    let src = "\
package main
import (
	\"fmt\"
	\"strconv\"
)
func main() {
	n, _ := strconv.Atoi(\"42\")
	fmt.Println(n + 8)
}
";
    assert_stdout(src, "50\n");
}

// ── go CLI subcommands ───────────────────────────────────────────────────────

/// Run the `go` binary with arbitrary args; return (stdout, success).
fn run_args(args: &[&str]) -> (String, bool) {
    let out = Command::new(env!("CARGO_BIN_EXE_go"))
        .args(args)
        .output()
        .expect("spawn go binary");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.success(),
    )
}

#[test]
fn cli_env_reports_goos_goarch() {
    let (out, ok) = run_args(&["env"]);
    assert!(ok);
    assert!(out.contains("GOARCH="), "env missing GOARCH: {out}");
    assert!(out.contains("GOVERSION="), "env missing GOVERSION: {out}");
}

#[test]
fn cli_vet_passes_clean_and_fails_broken() {
    let mut good = tempfile::Builder::new().suffix(".go").tempfile().unwrap();
    good.write_all(b"package main\nimport \"fmt\"\nfunc main() { fmt.Println(1) }\n")
        .unwrap();
    let (_o, ok) = run_args(&["vet", good.path().to_str().unwrap()]);
    assert!(ok, "vet should pass a clean program");

    let mut bad = tempfile::Builder::new().suffix(".go").tempfile().unwrap();
    bad.write_all(b"package main\nfunc main() { undefinedThing() }\n")
        .unwrap();
    let (_o, ok) = run_args(&["vet", bad.path().to_str().unwrap()]);
    assert!(!ok, "vet should fail an ill-formed program");
}

#[test]
fn cli_build_produces_a_runnable_native_binary() {
    // Needs a C compiler + libgors.a next to the binary; skip if either is absent.
    let lib = std::path::Path::new(env!("CARGO_BIN_EXE_go"))
        .parent()
        .unwrap()
        .join("libgors.a");
    if !lib.exists() || Command::new("cc").arg("--version").output().is_err() {
        return; // environment can't link; not a go-rs failure
    }
    let mut src = tempfile::Builder::new().suffix(".go").tempfile().unwrap();
    src.write_all(b"package main\nimport \"fmt\"\nfunc main() { fmt.Println(6 * 7) }\n")
        .unwrap();
    let out = std::env::temp_dir().join(format!("gors_test_build_{}", std::process::id()));
    let (_o, ok) = run_args(&[
        "build",
        "-o",
        out.to_str().unwrap(),
        src.path().to_str().unwrap(),
    ]);
    assert!(ok, "go build should succeed");
    let run = Command::new(&out).output().expect("run native binary");
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n");
    let _ = std::fs::remove_file(&out);
}

#[test]
fn multi_value_return_and_destructure() {
    let src = "\
package main
import \"fmt\"
func divmod(a int, b int) (int, int) {
	return a / b, a % b
}
func swap(a string, b string) (string, string) {
	return b, a
}
func main() {
	q, r := divmod(17, 5)
	x, y := swap(\"first\", \"second\")
	fmt.Println(q, r, x, y)
}
";
    assert_stdout(src, "3 2 second first\n");
}

#[test]
fn function_typed_parameters_dispatch_dynamically() {
    // A `func(int) int` parameter is called by value: `apply` doesn't know
    // statically whether it holds `double` or `inc`, so the call goes through
    // the closure's stored subroutine name-index (Op::CallDynamic).
    let src = "\
package main
import \"fmt\"
func apply(f func(int) int, x int) int {
	return f(x)
}
func reduce(nums []int, acc int, op func(int, int) int) int {
	for _, n := range nums {
		acc = op(acc, n)
	}
	return acc
}
func main() {
	double := func(n int) int { return n * 2 }
	inc := func(n int) int { return n + 1 }
	fmt.Println(apply(double, 21))
	fmt.Println(apply(inc, 41))
	add := func(a, b int) int { return a + b }
	fmt.Println(reduce([]int{1, 2, 3, 4, 5}, 0, add))
}
";
    assert_stdout(src, "42\n42\n15\n");
}

#[test]
fn func_value_captured_inside_a_lambda_is_callable() {
    // `compose` returns a closure that captures two func-typed params and calls
    // them — the captured values must dispatch dynamically from inside the lambda.
    let src = "\
package main
import \"fmt\"
func compose(f func(int) int, g func(int) int) func(int) int {
	return func(x int) int { return f(g(x)) }
}
func main() {
	double := func(n int) int { return n * 2 }
	inc := func(n int) int { return n + 1 }
	h := compose(double, inc)
	fmt.Println(h(10))
}
";
    assert_stdout(src, "22\n");
}

#[test]
fn go_doc_prints_reference_for_a_name() {
    // `go doc append` renders the builtin's category, description and example
    // from the same corpus that drives --lsp hover and docs/reference.html.
    let out = Command::new(env!("CARGO_BIN_EXE_go"))
        .args(["doc", "append"])
        .output()
        .expect("spawn go doc");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("append  (Builtin)"), "got: {text}");
    assert!(text.contains("example:"), "got: {text}");

    // An unknown name is an error on stderr with a non-zero exit.
    let bad = Command::new(env!("CARGO_BIN_EXE_go"))
        .args(["doc", "definitely-not-a-symbol"])
        .output()
        .expect("spawn go doc");
    assert!(!bad.status.success());
}

#[test]
fn generic_functions_erase_type_parameters() {
    // Type parameters and constraint interfaces are erased; the dynamic value
    // model runs the same code for int and float instantiations. `var total T`
    // (a type-parameter zero value) accumulates correctly for either.
    let src = "\
package main
import \"fmt\"
type Number interface{ ~int | ~float64 }
func Sum[T Number](xs []T) T {
	var total T
	for _, x := range xs {
		total += x
	}
	return total
}
func Map[T any, U any](xs []T, f func(T) U) []U {
	out := make([]U, 0)
	for _, x := range xs {
		out = append(out, f(x))
	}
	return out
}
func main() {
	fmt.Println(Sum([]int{1, 2, 3, 4, 5}))
	fmt.Println(Sum([]float64{1.5, 2.5, 3.0}))
	fmt.Println(Map([]int{1, 2, 3}, func(n int) int { return n * n }))
}
";
    assert_stdout(src, "15\n7\n[1 4 9]\n");
}

#[test]
fn generic_struct_type_and_methods() {
    // A generic struct `Stack[T]`, a pointer-receiver generic method, and both
    // inferred and explicit instantiation of a generic constructor.
    let src = "\
package main
import \"fmt\"
type Stack[T any] struct {
	items []T
}
func (s *Stack[T]) Push(x T) {
	s.items = append(s.items, x)
}
func (s *Stack[T]) Len() int {
	return len(s.items)
}
type Pair[K any, V any] struct {
	Key K
	Val V
}
func MakePair[K any, V any](k K, v V) Pair[K, V] {
	return Pair[K, V]{Key: k, Val: v}
}
func main() {
	var s Stack[int]
	s.Push(10)
	s.Push(20)
	fmt.Println(s.Len(), s.items)
	p := MakePair(1, \"one\")
	fmt.Println(p.Key, p.Val)
	q := Pair[string, int]{Key: \"age\", Val: 42}
	fmt.Println(q.Key, q.Val)
}
";
    assert_stdout(src, "2 [10 20]\n1 one\nage 42\n");
}

#[test]
fn defer_runs_lifo_with_snapshotted_args() {
    // Deferred calls run in LIFO order at function return; each `defer` snapshots
    // its arguments at defer time (so the loop prints 2, 1, 0 after the body).
    let src = "\
package main
import \"fmt\"
func work() {
	for i := 0; i < 3; i++ {
		defer fmt.Println(\"deferred\", i)
	}
	fmt.Println(\"body\")
}
func main() {
	work()
}
";
    assert_stdout(src, "body\ndeferred 2\ndeferred 1\ndeferred 0\n");
}

#[test]
fn defer_pointer_receiver_sees_later_mutations() {
    // A deferred pointer-receiver method captures the receiver by reference, so
    // it observes mutations made after the `defer` (Go captures the pointer).
    let src = "\
package main
import \"fmt\"
type Counter struct{ n int }
func (c *Counter) Inc()    { c.n++ }
func (c *Counter) Report() { fmt.Println(\"count:\", c.n) }
func run() {
	var c Counter
	defer c.Report()
	c.Inc()
	c.Inc()
	c.Inc()
}
func main() {
	run()
}
";
    assert_stdout(src, "count: 3\n");
}

#[test]
fn panic_recover_across_a_call_frame() {
    // A panic unwinds through a call, is caught by `recover()` in a deferred
    // closure of an enclosing function, and execution continues normally.
    let src = "\
package main
import \"fmt\"
func mightPanic(n int) {
	if n > 5 {
		panic(\"too big\")
	}
	fmt.Println(\"ok:\", n)
}
func guarded(n int) {
	defer func() {
		if r := recover(); r != nil {
			fmt.Println(\"caught:\", r)
		}
	}()
	mightPanic(n)
	fmt.Println(\"after\", n)
}
func main() {
	guarded(3)
	guarded(9)
	fmt.Println(\"done\")
}
";
    assert_stdout(src, "ok: 3\nafter 3\ncaught: too big\ndone\n");
}

#[test]
fn recovered_multi_value_call_returns_zero_values() {
    // A recovered call returns its result's zero values (the recover is observed
    // via a side effect). Deferred mutation of *named* results is a documented
    // gap pending capture-by-reference, so this asserts the zero-value return.
    let src = "\
package main
import \"fmt\"
func safe(a, b int) (int, string) {
	defer func() {
		if r := recover(); r != nil {
			fmt.Println(\"recovered:\", r)
		}
	}()
	if b == 0 {
		panic(\"div by zero\")
	}
	return a / b, \"ok\"
}
func main() {
	q, s := safe(10, 2)
	fmt.Println(q, s)
	q2, s2 := safe(1, 0)
	fmt.Println(q2, s2 == \"\")
}
";
    assert_stdout(src, "5 ok\nrecovered: div by zero\n0 true\n");
}

#[test]
fn closure_captures_variable_by_reference() {
    // A closure mutating a captured variable propagates the change (Go captures
    // the variable, not a copy): a counter closure, and two closures sharing one
    // captured variable.
    let src = "\
package main
import \"fmt\"
func makeCounter() func() int {
	count := 0
	return func() int {
		count++
		return count
	}
}
func main() {
	c := makeCounter()
	fmt.Println(c(), c(), c())
	total := 0
	add := func(n int) { total += n }
	get := func() int { return total }
	add(5)
	add(10)
	fmt.Println(get())
}
";
    assert_stdout(src, "1 2 3\n15\n");
}

#[test]
fn closure_mutation_observed_after_call() {
    // Mutating an enclosing local through a closure is visible in the enclosing
    // scope after the call returns.
    let src = "\
package main
import \"fmt\"
func main() {
	x := 1
	bump := func() { x = x * 10 }
	bump()
	bump()
	fmt.Println(x)
}
";
    assert_stdout(src, "100\n");
}

#[test]
fn loop_variable_capture_is_per_iteration() {
    // Go 1.22: each iteration has its own loop variable, so a closure created in
    // the loop captures that iteration's value (not the final one).
    let src = "\
package main
import \"fmt\"
func main() {
	f0 := func() int { return -1 }
	f1 := func() int { return -1 }
	f2 := func() int { return -1 }
	for i := 0; i < 3; i++ {
		f := func() int { return i }
		if i == 0 {
			f0 = f
		} else if i == 1 {
			f1 = f
		} else {
			f2 = f
		}
	}
	fmt.Println(f0(), f1(), f2())
}
";
    assert_stdout(src, "0 1 2\n");
}

#[test]
fn constant_float_expressions_fold_exactly() {
    // A compile-time-constant float expression is evaluated with Go's
    // arbitrary-precision rounding (round once), not runtime f64 double-rounding.
    // `1.950 * 10.187` and `0.1 + 0.2` are the classic differing cases.
    let src = "\
package main
import \"fmt\"
func main() {
	fmt.Printf(\"%.10f\\n\", 1.950*10.187)
	fmt.Println(0.1 + 0.2)
	fmt.Println(2.5 * 4.0)
	fmt.Printf(\"%.10f\\n\", 100.0/8.0)
}
";
    assert_stdout(src, "19.8646500000\n0.3\n10\n12.5000000000\n");
}

#[test]
fn sprintf_and_sprint() {
    let src = "\
package main
import \"fmt\"
func main() {
	s := fmt.Sprintf(\"%d-%s-%.2f\", 42, \"go\", 3.14159)
	fmt.Println(s, len(s))
	fmt.Println(fmt.Sprint(\"a\", 1, \"b\"))
}
";
    assert_stdout(src, "42-go-3.14 10\na1b\n");
}

#[test]
fn call_func_value_from_index() {
    let src = "\
package main
import \"fmt\"
func main() {
	fns := []func(int) int{}
	for i := 0; i < 3; i++ {
		fns = append(fns, func(x int) int { return x + i })
	}
	fmt.Println(fns[0](10), fns[1](10), fns[2](10))
	ops := map[string]func(int, int) int{
		\"add\": func(a, b int) int { return a + b },
		\"mul\": func(a, b int) int { return a * b },
	}
	fmt.Println(ops[\"add\"](3, 4), ops[\"mul\"](3, 4))
}
";
    assert_stdout(src, "10 11 12\n7 12\n");
}

#[test]
fn slice_expressions_on_slices_and_strings() {
    let src = "\
package main
import \"fmt\"
func main() {
	xs := []int{10, 20, 30, 40, 50}
	fmt.Println(xs[1:3], xs[:2], xs[3:], xs[:])
	s := \"hello, world\"
	fmt.Println(s[0:5], s[7:])
	stack := []int{1, 2, 3}
	top := stack[len(stack)-1]
	stack = stack[:len(stack)-1]
	fmt.Println(top, stack)
}
";
    assert_stdout(
        src,
        "[20 30] [10 20] [40 50] [10 20 30 40 50]\nhello world\n3 [1 2]\n",
    );
}

#[test]
fn address_of_shares_the_struct() {
    let src = "\
package main
import \"fmt\"
type Counter struct{ n int }
func (c *Counter) Inc() { c.n++ }
func newCounter() *Counter { return &Counter{n: 0} }
func main() {
	c := &Counter{n: 5}
	c.Inc()
	fmt.Println(c.n)
	d := newCounter()
	d.Inc()
	fmt.Println(d.n)
	e := Counter{n: 10}
	p := &e
	p.Inc()
	fmt.Println(e.n)
}
";
    assert_stdout(src, "6\n1\n11\n");
}

#[test]
fn switch_tag_tagless_and_break_continue() {
    let src = "\
package main
import \"fmt\"
func describe(n int) string {
	switch n {
	case 0:
		return \"zero\"
	case 1, 2, 3:
		return \"small\"
	default:
		return \"other\"
	}
}
func grade(s int) string {
	switch {
	case s >= 90:
		return \"A\"
	case s >= 80:
		return \"B\"
	default:
		return \"F\"
	}
}
func main() {
	fmt.Println(describe(0), describe(2), describe(9))
	fmt.Println(grade(95), grade(85), grade(50))
	total := 0
	for i := 0; i < 5; i++ {
		switch i {
		case 2:
			continue
		case 4:
			break
		}
		total += i
	}
	fmt.Println(total)
}
";
    assert_stdout(src, "zero small other\nA B F\n8\n");
}

#[test]
fn named_return_values() {
    let src = "\
package main
import \"fmt\"
func divmod(a, b int) (q, r int) {
	q = a / b
	r = a % b
	return
}
func withDefer(x int) (result int) {
	defer func() { result = result * 2 }()
	result = x + 1
	return
}
func safe(a, b int) (n int, err string) {
	defer func() {
		if r := recover(); r != nil {
			err = \"recovered\"
		}
	}()
	if b == 0 {
		panic(\"boom\")
	}
	return a / b, \"\"
}
func main() {
	q, r := divmod(17, 5)
	fmt.Println(q, r)
	fmt.Println(withDefer(4))
	n, e := safe(10, 2)
	fmt.Println(n, e)
	n2, e2 := safe(1, 0)
	fmt.Println(n2, e2)
}
";
    assert_stdout(src, "3 2\n10\n5 \n0 recovered\n");
}

#[test]
fn parallel_assignment() {
    // Right-hand sides are evaluated before any assignment, so a swap and a
    // rotate work; also multi-return into existing vars and index/map targets.
    let src = "\
package main
import \"fmt\"
func vals() (int, int) { return 8, 9 }
func main() {
	a, b := 1, 2
	a, b = b, a
	fmt.Println(a, b)
	x, y, z := 10, 20, 30
	x, y, z = z, x, y
	fmt.Println(x, y, z)
	p, q := 0, 0
	p, q = vals()
	fmt.Println(p, q)
	m := map[string]int{\"k\": 1}
	arr := []int{0, 0}
	m[\"k\"], arr[1] = 5, 7
	fmt.Println(m[\"k\"], arr[1])
}
";
    assert_stdout(src, "2 1\n30 10 20\n8 9\n5 7\n");
}

#[test]
fn runtime_panics_are_recoverable() {
    // A runtime fault (divide-by-zero, index-out-of-range, nil dereference) in a
    // program that uses recover() becomes a catchable panic; recover() returns
    // the Go runtime-error string.
    let src = "\
package main
import \"fmt\"
func try(f func()) (msg string) {
	defer func() {
		if r := recover(); r != nil {
			msg = fmt.Sprint(r)
		}
	}()
	f()
	return \"ok\"
}
func main() {
	fmt.Println(try(func() { xs := []int{1}; _ = xs[9] }))
	fmt.Println(try(func() { a, b := 1, 0; _ = a / b }))
	fmt.Println(try(func() { fmt.Print(\"\") }))
}
";
    assert_stdout(
        src,
        "runtime error: index out of range [9] with length 1\nruntime error: integer divide by zero\nok\n",
    );
}

#[test]
fn unrecovered_runtime_panic_aborts() {
    // Without recover, a runtime fault aborts (non-zero exit); output before the
    // fault is still produced.
    let (stdout, ok) = run(
        "package main\nimport \"fmt\"\nfunc main() {\n\tfmt.Println(\"before\")\n\ta, b := 1, 0\n\tfmt.Println(a / b)\n\tfmt.Println(\"after\")\n}\n",
    );
    assert!(!ok, "unrecovered runtime panic should exit non-zero");
    assert_eq!(stdout, "before\n");
}

#[test]
fn multi_value_spread_into_call() {
    // `f(g())` where g returns multiple values passes them as f's arguments.
    let src = "\
package main
import \"fmt\"
func pair() (int, string) { return 42, \"go\" }
func triple() (int, int, int) { return 1, 2, 3 }
func add(a, b, c int) int { return a + b + c }
func main() {
	fmt.Println(pair())
	fmt.Println(add(triple()))
}
";
    assert_stdout(src, "42 go\n6\n");
}

#[test]
fn sub_slice_shares_backing_array() {
    // A sub-slice `s[lo:hi]` shares the parent's backing array: element writes
    // are visible through the parent (and via nested sub-slices), len/cap reflect
    // the offset, and append writes in place when the backing has spare capacity.
    let src = "\
package main
import \"fmt\"
func main() {
	a := []int{5, 3, 8, 1, 9, 2}
	mid := a[1:5]
	fmt.Println(mid, len(mid), cap(mid))
	mid[0] = 100
	inner := mid[1:3]
	inner[0] = 200
	fmt.Println(a)
	b := []int{1, 2, 3, 4, 5}
	c := b[0:2]
	c = append(c, 99)
	fmt.Println(b, c)
}
";
    assert_stdout(
        src,
        "[3 8 1 9] 4 5\n[5 100 200 1 9 2]\n[1 2 99 4 5] [1 2 99]\n",
    );
}

#[test]
fn bitwise_operators_and_base_literals() {
    let src = "\
package main
import \"fmt\"
func main() {
	a, b := 12, 10
	fmt.Println(a&b, a|b, a^b, a&^b, ^a)
	fmt.Println(1<<4, 256>>2, 1|2&3)
	fmt.Println(0xFF, 0o17, 0b1010, 1_000_000)
	x := 1
	x |= 6
	x <<= 2
	fmt.Println(x)
}
";
    assert_stdout(src, "8 14 6 4 -13\n16 64 3\n255 15 10 1000000\n28\n");
}

#[test]
fn type_conversions() {
    let src = "\
package main
import \"fmt\"
func main() {
	f := 3.9
	fmt.Println(int(f), float64(10)/4)
	n := 65
	fmt.Println(string(rune(n)))
}
";
    assert_stdout(src, "3 2.5\nA\n");
}

#[test]
fn const_blocks_and_iota() {
    let src = "\
package main
import \"fmt\"
type Flag int
const (
	Read Flag = 1 << iota
	Write
	Exec
)
const (
	_  = iota
	KB = 1 << (10 * iota)
	MB
)
func main() {
	fmt.Println(Read, Write, Exec)
	fmt.Println(KB, MB)
	const local = 42
	fmt.Println(local)
}
";
    assert_stdout(src, "1 2 4\n1024 1048576\n42\n");
}

#[test]
fn variadic_functions_and_spread() {
    let src = "\
package main
import \"fmt\"
func sum(nums ...int) int {
	total := 0
	for _, n := range nums {
		total += n
	}
	return total
}
func tag(prefix string, rest ...string) string {
	out := prefix
	for _, s := range rest {
		out += \"-\" + s
	}
	return out
}
func main() {
	fmt.Println(sum(1, 2, 3, 4, 5), sum(), sum(10))
	xs := []int{6, 7, 8}
	fmt.Println(sum(xs...))
	fmt.Println(tag(\"a\"), tag(\"a\", \"b\", \"c\"))
	parts := []string{\"x\", \"y\"}
	fmt.Println(tag(\"p\", parts...))
}
";
    assert_stdout(src, "15 0 10\n21\na a-b-c\np-x-y\n");
}

#[test]
fn func_typed_parameter_is_not_captured_by_an_unrelated_closure_name() {
    // `closure_vars` maps a name to the lambda it was bound to, for static
    // dispatch. It used to survive the end of a function body, so `apply`'s
    // parameter `f` dispatched to whatever lambda `main` had bound to `f` —
    // silently answering with the wrong closure rather than failing.
    let src = "\
package main
import \"fmt\"
func apply(f func(int) int, v int) int { return f(v) }
func twice(f func(int) int, v int) int { return f(f(v)) }
func main() {
	f := func(n int) int { return n * 10 }
	fmt.Println(apply(func(n int) int { return n + 1 }, 10))
	fmt.Println(twice(func(n int) int { return n + 1 }, 10))
	fmt.Println(f(10))
}
";
    assert_stdout(src, "11\n12\n100\n");
}

#[test]
fn multi_value_return_through_a_func_value() {
    // The result count of a call through a func value comes from the literal's
    // own declared results. Without it the returned tuple stayed a single value
    // and `q, r := dm(17, 5)` bound the whole slice to `q`.
    let src = "\
package main
import \"fmt\"
func divmod(a, b int) (int, int) { return a / b, a % b }
func main() {
	lit := func(a, b int) (int, int) { return a + b, a - b }
	s, d := lit(9, 4)
	fmt.Println(s, d)
	dm := divmod
	q, r := dm(17, 5)
	fmt.Println(q, r)
	three := func() (int, string, bool) { return 1, \"two\", true }
	a, b, c := three()
	fmt.Println(a, b, c)
}
";
    assert_stdout(src, "13 5\n3 2\n1 two true\n");
}

#[test]
fn declared_function_used_as_a_value() {
    // A declared function named in a value position is a function value. It had
    // no lowering at all — `apply(dbl)` read `dbl` as a variable and failed with
    // "call of a nil or unknown function value" at run time.
    let src = "\
package main
import \"fmt\"
func dbl(n int) int { return n * 2 }
func inc(n int) int { return n + 1 }
func apply(f func(int) int, v int) int { return f(v) }
func main() {
	fmt.Println(apply(dbl, 21), apply(inc, 41))
	f := dbl
	fmt.Println(f(5), f == nil)
	fns := []func(int) int{dbl, inc}
	fmt.Println(fns[0](10), fns[1](10))
	dbl := 99
	fmt.Println(dbl)
}
";
    assert_stdout(src, "42 42\n10 false\n20 11\n99\n");
}

#[test]
fn variadic_closure_packs_its_trailing_arguments() {
    // A closure's variadic parameter binds a *slice* of the trailing arguments,
    // like a declared function's. Before the flag survived to the call site the
    // literal's parameters bound one operand off: `p("none")` printed the
    // closure handle as `tag` and read a length of 2.
    let src = "\
package main
import \"fmt\"
func main() {
	p := func(tag string, a ...any) { fmt.Println(tag, len(a), a) }
	p(\"none\")
	p(\"many\", 1, \"x\")
	only := func(ns ...int) int { return len(ns) }
	xs := []int{5, 6, 7}
	fmt.Println(only(), only(1, 2), only(xs...))
	func(pre string, ys ...string) { fmt.Println(pre, len(ys), ys) }(\"iife\", \"a\", \"b\")
	out := make(chan string)
	go func(tag string, ns ...int) { out <- fmt.Sprint(tag, len(ns), ns) }(\"go\", 1, 2, 3)
	fmt.Println(<-out)
}
";
    assert_stdout(
        src,
        "none 0 []\nmany 2 [1 x]\n0 2 3\niife 2 [a b]\ngo3 [1 2 3]\n",
    );
}

#[test]
fn switch_fallthrough() {
    let src = "\
package main
import \"fmt\"
func describe(n int) string {
	s := \"\"
	switch n {
	case 1:
		s += \"one \"
		fallthrough
	case 2:
		s += \"two \"
		fallthrough
	case 3:
		s += \"three \"
	default:
		s += \"other \"
	}
	return s
}
func main() {
	fmt.Println(describe(1))
	fmt.Println(describe(2))
	fmt.Println(describe(3))
	fmt.Println(describe(9))
}
";
    assert_stdout(src, "one two three \ntwo three \nthree \nother \n");
}

#[test]
fn type_switch_and_assertions() {
    let src = "\
package main
import \"fmt\"
type Point struct{ x int }
func kind(v any) string {
	switch t := v.(type) {
	case int:
		return fmt.Sprintf(\"int %d\", t)
	case string:
		return \"str \" + t
	case Point:
		return fmt.Sprintf(\"point %d\", t.x)
	default:
		return \"?\"
	}
}
func main() {
	fmt.Println(kind(5), kind(\"hi\"), kind(Point{x: 9}), kind(true))
	var i any = \"go\"
	s, ok := i.(string)
	fmt.Println(s, ok)
	n, ok2 := i.(int)
	fmt.Println(n, ok2)
	var j any = 7
	fmt.Println(j.(int) + 1)
}
";
    assert_stdout(src, "int 5 str hi point 9 ?\ngo true\n0 false\n8\n");
}

#[test]
fn imports_errors_package_from_source() {
    // The `errors` package is not a native builtin — it is loaded from its real
    // Go source (vendored), name-qualified, and linked into the program.
    let src = "\
package main
import (
	\"fmt\"
	\"errors\"
)
func main() {
	err := errors.New(\"boom\")
	fmt.Println(err.Error())
	var e error = err
	fmt.Println(e.Error())
}
";
    assert_stdout(src, "boom\nboom\n");
}

#[test]
fn fmt_calls_error_and_stringer_methods() {
    // fmt prints a value implementing error/Stringer via its method (Go's fmt
    // interface handling), synthesized as `$stringify` and wrapped around args.
    let src = "\
package main
import \"fmt\"
type Color struct{ r, g, b int }
func (c Color) String() string { return fmt.Sprintf(\"#%02x%02x%02x\", c.r, c.g, c.b) }
type myErr struct{ msg string }
func (e *myErr) Error() string { return e.msg }
func main() {
	c := Color{255, 128, 0}
	fmt.Println(c)
	fmt.Printf(\"%v %s\\n\", c, c)
	var err error = &myErr{\"nope\"}
	fmt.Println(err)
	fmt.Println(\"plain\", 42, true)
}
";
    assert_stdout(src, "#ff8000\n#ff8000 #ff8000\nnope\nplain 42 true\n");
}

#[test]
fn new_builtin_allocates_zero_pointer() {
    // `new(T)` allocates a zero value of T and returns a pointer to it: a struct
    // lowers to `&T{}` (zero-filled), a scalar to a pointer to its zero value.
    let src = "\
package main
import \"fmt\"
type P struct{ x, y int }
func (p *P) Bump() { p.x++ }
func main() {
	p := new(P)
	p.Bump()
	p.Bump()
	fmt.Println(p.x, p.y)
	n := new(int)
	fmt.Println(*n)
}
";
    assert_stdout(src, "2 0\n0\n");
}

#[test]
fn rune_literals_are_int_code_points() {
    // A Go rune is int32: a rune literal is its Unicode code point, so it prints
    // as an integer, does arithmetic, and compares equal to a range/index value
    // (both are code points). Escapes cover \n \xHH \uHHHH \U... and octal.
    let src = "\
package main
import \"fmt\"
func main() {
	fmt.Println('A')
	fmt.Println('A' + 1)
	var r rune = 'a'
	fmt.Println(r - 'a')
	fmt.Println('z' - '0')
	fmt.Println(string(rune(65)))
	for _, c := range \"cat\" {
		if c == 'a' {
			fmt.Println(\"found a\")
		}
	}
	fmt.Println('\\n', '\\x41', '\\u00e9')
}
";
    assert_stdout(src, "65\n66\n0\n74\nA\nfound a\n10 65 233\n");
}

#[test]
fn byte_and_rune_slice_conversions() {
    // []byte(s) yields the UTF-8 bytes; []rune(s) yields the code points; and
    // string() converts each back — a []byte is UTF-8-decoded, a []rune is
    // code-point-joined (go-rs erases the element type, so string() decides by
    // whether the bytes form valid multibyte UTF-8).
    let src = "\
package main
import \"fmt\"
func main() {
	s := \"AB\\u00e9\"
	b := []byte(s)
	fmt.Println(b, len(b))
	r := []rune(s)
	fmt.Println(r, len(r))
	fmt.Println(string(b))
	fmt.Println(string(r))
	fmt.Println(string([]rune{72, 233, 108, 108, 111}))
}
";
    assert_stdout(
        src,
        "[65 66 195 169] 4\n[65 66 233] 3\nAB\u{e9}\nAB\u{e9}\nH\u{e9}llo\n",
    );
}

#[test]
fn large_hex_literals_wrap_to_bit_pattern() {
    // A base-prefixed constant above i64::MAX (a uint64 bit mask) is stored as
    // the i64 with the same bit pattern, so bitwise use matches Go.
    let src = "\
package main
import \"fmt\"
func main() {
	const mask = 0x8080808080808080
	fmt.Println(mask & 0xFF)
	fmt.Println(0x0A&0x0F, 0xFF00>>8, 0o17, 0b1010)
}
";
    assert_stdout(src, "128\n10 255 15 10\n");
}

#[test]
fn string_literal_escapes() {
    // Interpreted string literals decode the full Go escape set: \xHH byte,
    // \uHHHH and \UHHHHHHHH Unicode, \ooo octal, and simple char escapes.
    let src = "\
package main
import \"fmt\"
func main() {
	fmt.Println(\"tab\\there\")
	fmt.Println(\"A=\\x41 e=\\u00e9 smile=\\U0001F600\")
	fmt.Println(\"octal-A=\\101\")
}
";
    assert_stdout(src, "tab\there\nA=A e=\u{e9} smile=\u{1F600}\noctal-A=A\n");
}

#[test]
fn fixed_size_array_literals() {
    // Arrays are modeled as slices: sequential [N]T, element-sized [...]T, sparse
    // index-keyed [N]T{i: v} with zero-fill, struct elements, and range/index.
    // Bare `var buf [N]scalar` zero-fills to N elements.
    let src = "\
package main
import \"fmt\"
type pair struct{ lo, hi int }
func main() {
	a := [3]int{10, 20, 30}
	fmt.Println(a, len(a), a[1])
	b := [...]int{1, 2, 3, 4}
	fmt.Println(b, len(b))
	c := [5]int{0: 100, 2: 300}
	fmt.Println(c)
	d := [4]pair{0: {1, 2}, 1: {3, 4}}
	fmt.Println(d)
	var buf [4]byte
	buf[1] = 9
	fmt.Println(buf)
	sum := 0
	for _, v := range c {
		sum += v
	}
	fmt.Println(sum)
}
";
    assert_stdout(
        src,
        "[10 20 30] 3 20\n[1 2 3 4] 4\n[100 0 300 0 0]\n[{1 2} {3 4} {0 0} {0 0}]\n[0 9 0 0]\n400\n",
    );
}

#[test]
fn three_index_slice_expression() {
    // A full slice expression s[low:high:max] caps the result at `max - low`, so
    // an append that would pass it reallocates instead of writing into backing
    // the sub-slice no longer owns.
    let src = "\
package main
import \"fmt\"
func main() {
	s := []int{1, 2, 3, 4, 5}
	fmt.Println(s[1:3:4], len(s[1:3:4]), cap(s[1:3:4]))
	fmt.Println(s[:2:5], cap(s[:2:5]), cap(s[:2]))
	t := s[0:2:2]
	t = append(t, 99)
	fmt.Println(t, s)
}
";
    assert_stdout(src, "[2 3] 2 3\n[1 2] 5 5\n[1 2 99] [1 2 3 4 5]\n");
}

#[test]
fn range_over_string_yields_runes() {
    // Go ranges a string by rune: the index is each rune's start byte offset and
    // the value is the code point, so a multibyte string iterates once per rune.
    let src = "\
package main
import \"fmt\"
func main() {
	for i, c := range \"h\\u00e9llo\" {
		fmt.Println(i, c)
	}
	n := 0
	for range \"h\\u00e9llo\" {
		n++
	}
	fmt.Println(\"count\", n)
}
";
    assert_stdout(src, "0 104\n1 233\n3 108\n4 108\n5 111\ncount 5\n");
}

#[test]
fn fmt_errorf_builds_an_error_value() {
    // fmt.Errorf formats a message into a real error value: it prints as the
    // message, its Error() method returns the message, and it satisfies `error`.
    let src = "\
package main
import \"fmt\"
func main() {
	e := fmt.Errorf(\"bad input: %d (%s)\", 42, \"oops\")
	fmt.Println(e)
	fmt.Println(e.Error())
	var err error = fmt.Errorf(\"code %d\", 7)
	fmt.Printf(\"got: %v\\n\", err)
}
";
    assert_stdout(
        src,
        "bad input: 42 (oops)\nbad input: 42 (oops)\ngot: code 7\n",
    );
}

#[test]
fn copy_builtin() {
    // copy(dst, src) copies min(len(dst), len(src)) elements and returns the
    // count; src may be a slice or a string (copy([]byte, s)).
    let src = "\
package main
import \"fmt\"
func main() {
	dst := make([]int, 3)
	n := copy(dst, []int{7, 8, 9, 10})
	fmt.Println(dst, n)
	buf := make([]byte, 5)
	m := copy(buf, \"hello world\")
	fmt.Println(m, string(buf))
}
";
    assert_stdout(src, "[7 8 9] 3\n5 hello\n");
}

#[test]
fn package_globals_visible_in_functions() {
    // Package-level consts/vars are readable and writable from any function, not
    // just main. A local declaration shadows a global without clobbering it.
    let src = "\
package main
import \"fmt\"
var counter = 100
const base = 10
func inc()          { counter++ }
func readGlobal() int { return counter + base }
func shadow() int {
	counter := 5
	counter += base
	return counter
}
func main() {
	inc()
	inc()
	fmt.Println(counter)
	fmt.Println(readGlobal())
	fmt.Println(shadow())
	fmt.Println(counter)
}
";
    assert_stdout(src, "102\n112\n15\n102\n");
}

#[test]
fn composite_literal_in_control_header_needs_parens() {
    // A bare `T{…}` is suppressed in an if/for/switch header (a `{` there opens
    // the body); parentheses re-enable it. Normal composite positions still work.
    let src = "\
package main
import \"fmt\"
type P struct{ x, y int }
func main() {
	p := P{x: 3, y: 4}
	fmt.Println(p.x, p.y)
	if (P{x: 5}).x > 0 {
		fmt.Println(\"pos\")
	}
	for i := 0; i < (P{x: 2}).x; i++ {
		fmt.Print(i, \" \")
	}
	fmt.Println()
}
";
    assert_stdout(src, "3 4\npos\n0 1 \n");
}

#[test]
fn map_comma_ok_lookup() {
    // `v, ok := m[k]` reports whether the key was present, distinct from a zero
    // value; also works in an if-init clause.
    let src = "\
package main
import \"fmt\"
func main() {
	m := map[string]int{\"a\": 1, \"b\": 0}
	v, ok := m[\"a\"]
	fmt.Println(v, ok)
	z, ok2 := m[\"b\"]
	fmt.Println(z, ok2)
	miss, ok3 := m[\"x\"]
	fmt.Println(miss, ok3)
	if _, found := m[\"b\"]; found {
		fmt.Println(\"has b\")
	}
}
";
    assert_stdout(src, "1 true\n0 true\n0 false\nhas b\n");
}

#[test]
fn append_spread_expands_slice() {
    // append(base, xs...) spreads every element of xs, incl. slice-expression
    // arguments (element removal via append(s[:i], s[i+1:]...)).
    let src = "\
package main
import \"fmt\"
func main() {
	a := []int{1, 2}
	b := []int{3, 4, 5}
	fmt.Println(append(a, b...))
	s := []int{10, 20, 30, 40}
	fmt.Println(append(s[:1], s[2:]...))
}
";
    assert_stdout(src, "[1 2 3 4 5]\n[10 30 40]\n");
}

#[test]
fn method_multi_value_return_destructures() {
    // v, ok := recv.M() destructures a multi-value method return (not only a
    // top-level func).
    let src = "\
package main
import \"fmt\"
type C struct{ n int }
func (c *C) Two() (int, bool) { return c.n, c.n > 0 }
func main() {
	c := &C{5}
	v, ok := c.Two()
	fmt.Println(v, ok)
}
";
    assert_stdout(src, "5 true\n");
}

#[test]
fn anonymous_struct_types() {
    // struct{…} as a map value (empty-struct set), as a slice element with elided
    // literals and field access, and as an inline composite value.
    let src = "\
package main
import \"fmt\"
func main() {
	set := map[string]struct{}{}
	set[\"a\"] = struct{}{}
	_, ok := set[\"a\"]
	_, no := set[\"z\"]
	fmt.Println(len(set), ok, no)
	tests := []struct {
		name string
		val  int
	}{
		{\"x\", 1},
		{\"y\", 2},
	}
	for _, t := range tests {
		fmt.Println(t.name, t.val)
	}
	p := struct{ a, b int }{10, 20}
	fmt.Println(p.a + p.b)
}
";
    assert_stdout(src, "1 true false\nx 1\ny 2\n30\n");
}

#[test]
fn struct_values_as_map_keys() {
    // A struct key compares by value (field-by-field), not heap identity: insert,
    // overwrite, lookup, comma-ok miss, and delete all key on the struct's fields.
    let src = "\
package main
import \"fmt\"
type P struct{ x, y int }
func main() {
	m := map[P]string{}
	m[P{1, 2}] = \"a\"
	m[P{3, 4}] = \"b\"
	m[P{1, 2}] = \"A\"
	fmt.Println(len(m), m[P{1, 2}], m[P{3, 4}])
	_, ok := m[P{9, 9}]
	fmt.Println(ok)
	delete(m, P{3, 4})
	fmt.Println(len(m))
}
";
    assert_stdout(src, "2 A b\nfalse\n1\n");
}

#[test]
fn sort_slice_with_closure_comparator() {
    // sort.Slice / sort.SliceStable sort in place via a closure comparator
    // (lowered to a synthesized in-language insertion sort that calls `less`).
    let src = "\
package main
import (
	\"fmt\"
	\"sort\"
)
func main() {
	s := []int{5, 2, 8, 1, 9}
	sort.Slice(s, func(i, j int) bool { return s[i] < s[j] })
	fmt.Println(s)
	sort.Slice(s, func(i, j int) bool { return s[i] > s[j] })
	fmt.Println(s)
	w := []string{\"banana\", \"apple\", \"cherry\"}
	sort.SliceStable(w, func(i, j int) bool { return w[i] < w[j] })
	fmt.Println(w)
}
";
    assert_stdout(src, "[1 2 5 8 9]\n[9 8 5 2 1]\n[apple banana cherry]\n");
}

#[test]
fn labeled_break_and_continue() {
    // A label names an enclosing `for` or `switch`; `continue L` restarts that
    // loop from a nested one, `break L` leaves it — including from inside a
    // `switch`, which an unlabeled `break` would only leave the switch of.
    // Verified against go 1.26.5.
    let src = "\
package main
import \"fmt\"
func main() {
outer:
	for i := 0; i < 4; i++ {
		for j := 0; j < 4; j++ {
			if j == 2 {
				continue outer
			}
			if i == 3 {
				break outer
			}
			fmt.Println(i, j)
		}
	}
loop:
	for i := range 5 {
		switch i {
		case 2:
			continue loop
		case 4:
			break loop
		}
		fmt.Println(\"i =\", i)
	}
	n := 0
sw:
	switch n {
	case 0:
		for k := 0; k < 3; k++ {
			if k == 1 {
				break sw
			}
			fmt.Println(\"k\", k)
		}
		fmt.Println(\"unreachable\")
	}
	fmt.Println(\"done\")
}
";
    assert_stdout(
        src,
        "0 0\n0 1\n1 0\n1 1\n2 0\n2 1\ni = 0\ni = 1\ni = 3\nk 0\ndone\n",
    );
}

#[test]
fn label_on_a_non_loop_is_rejected() {
    // A label can only introduce a `for` or a `switch`, so labeling anything
    // else is a compile error rather than a label that binds to nothing.
    let src = "\
package main
func main() {
lbl:
	x := 1
	_ = x
}
";
    let (out, ok) = run_capturing_stderr(src);
    assert!(!ok, "program unexpectedly succeeded: {out:?}");
    assert!(out.contains("label `lbl`"), "unexpected error: {out}");
}

#[test]
fn elided_composite_literal_element_types() {
    // Inside a composite literal the element type may be elided, for container
    // element types (`[][]int{{1, 2}}`) as well as struct ones. Verified
    // against go 1.26.5.
    let src = "\
package main
import \"fmt\"
type Point struct{ X, Y int }
func main() {
	fmt.Println([][]int{{1, 2}, {3, 4, 5}, {}})
	fmt.Println([]Point{{1, 2}, {X: 3}, {}})
	fmt.Println(map[string][]int{\"a\": {1, 2}}[\"a\"])
	fmt.Println(map[string]map[string]int{\"x\": {\"i\": 1}}[\"x\"][\"i\"])
	fmt.Println(map[string]Point{\"o\": {1, 2}}[\"o\"].Y)
	fmt.Println([2][2]int{{1, 2}, {3, 4}})
	fmt.Println([][][]int{{{1}, {2, 3}}, {{4}}})
	fmt.Println([4][]int{2: {7, 8}})
	fmt.Println(map[Point]string{{1, 2}: \"a\"}[Point{1, 2}])
}
";
    assert_stdout(
        src,
        "[[1 2] [3 4 5] []]\n[{1 2} {3 0} {0 0}]\n[1 2]\n1\n2\n[[1 2] [3 4]]\n\
         [[[1] [2 3]] [[4]]]\n[[] [] [7 8] []]\na\n",
    );
}

#[test]
fn embedded_struct_promotes_fields_and_methods() {
    // An embedded field's fields and methods are promoted onto the outer
    // struct, through more than one level; a method the outer type declares
    // itself shadows the promoted one, and a promoted field assignment writes
    // through to the embedded value. Verified against go 1.26.5.
    let src = "\
package main
import \"fmt\"
type Base struct {
	N    int
	Name string
}
func (b Base) Describe() string { return fmt.Sprintf(\"Base(%d,%s)\", b.N, b.Name) }
func (b Base) Double() int      { return b.N * 2 }
type Middle struct {
	Base
	Tag string
}
func (m Middle) Describe() string { return \"Middle:\" + m.Tag }
type Derived struct {
	Middle
	Extra int
}
type Describer interface{ Describe() string }
func main() {
	d := Derived{Middle: Middle{Base: Base{N: 3, Name: \"x\"}, Tag: \"t\"}, Extra: 9}
	fmt.Println(d.N, d.Name, d.Tag, d.Extra)
	fmt.Println(d.Describe(), d.Double(), d.Base.Describe(), d.Middle.Base.N)
	d.N = 42
	fmt.Println(d.N, d.Base.N, d.Double())
	var i Describer = d
	fmt.Println(i.Describe())
	i = d.Base
	fmt.Println(i.Describe())
	fmt.Println(d)
}
";
    assert_stdout(
        src,
        "3 x t 9\nMiddle:t 6 Base(3,x) 3\n42 42 84\nMiddle:t\nBase(42,x)\n{{{42 x} t} 9}\n",
    );
}

#[test]
fn embedded_pointer_and_shadowed_field_name() {
    // An embedded `*T` promotes through the pointer (so a pointer-receiver
    // method mutates the shared value), and a struct may hold both an embedded
    // `Base` and a named field of the same type without them colliding.
    // Verified against go 1.26.5.
    let src = "\
package main
import \"fmt\"
type Animal struct{ Legs int }
func (a *Animal) AddLeg()      { a.Legs++ }
func (a Animal) Walk() string  { return fmt.Sprintf(\"walking on %d\", a.Legs) }
type Dog struct {
	*Animal
	Name string
}
type Base struct{ ID int }
func (b *Base) Bump() { b.ID += 10 }
type Pair struct {
	Base
	B Base
}
func main() {
	d := Dog{Animal: &Animal{Legs: 4}, Name: \"rex\"}
	fmt.Println(d.Legs, d.Name, d.Walk())
	d.AddLeg()
	fmt.Println(d.Legs, d.Animal.Legs)
	p := Pair{Base{5}, Base{6}}
	fmt.Println(p.ID, p.B.ID, p)
	p.Bump()
	fmt.Println(p.ID, p.B.ID)
}
";
    assert_stdout(src, "4 rex walking on 4\n5 5\n5 6 {{5} {6}}\n15 6\n");
}

#[test]
fn errors_is_as_unwrap_and_wrap_verb() {
    // %w records the cause, so Is walks the chain; a plain %v does not. Two
    // errors with the same text are distinct values, which is what makes
    // sentinel matching mean anything.
    let src = "\
package main
import (
	\"errors\"
	\"fmt\"
)
var ErrGone = errors.New(\"gone\")
type codeErr struct{ code int }
func (e *codeErr) Error() string { return \"code\" }
func main() {
	wrapped := fmt.Errorf(\"step: %w\", ErrGone)
	plain := fmt.Errorf(\"step: %v\", ErrGone)
	fmt.Println(wrapped, errors.Is(wrapped, ErrGone), errors.Is(plain, ErrGone))
	fmt.Println(errors.Unwrap(wrapped) == ErrGone, errors.Unwrap(ErrGone))
	fmt.Println(errors.New(\"gone\") == errors.New(\"gone\"))
	deep := fmt.Errorf(\"outer: %w\", wrapped)
	fmt.Println(errors.Is(deep, ErrGone))
	ce := &codeErr{7}
	var got *codeErr
	fmt.Println(errors.As(fmt.Errorf(\"ctx: %w\", ce), &got), got.code)
	var miss *codeErr
	fmt.Println(errors.As(ErrGone, &miss), miss == nil)
	joined := errors.Join(ErrGone, ce)
	fmt.Println(errors.Is(joined, ErrGone), errors.Is(joined, ce))
}
";
    assert_stdout(
        src,
        "step: gone true false\ntrue <nil>\nfalse\ntrue\ntrue 7\nfalse true\ntrue true\n",
    );
}

#[test]
fn anonymous_interface_assertion_tests_the_method_set() {
    // `err.(interface{ Unwrap() error })` must match only types that really
    // have that method — and must tell `Unwrap() error` from `Unwrap() []error`,
    // which is the distinction errors.Is depends on.
    let src = "\
package main
import \"fmt\"
type leaf struct{}
func (leaf) Error() string { return \"leaf\" }
type one struct{}
func (one) Error() string { return \"one\" }
func (one) Unwrap() error { return leaf{} }
type many struct{}
func (many) Error() string   { return \"many\" }
func (many) Unwrap() []error { return []error{leaf{}} }
func kind(err error) string {
	switch x := err.(type) {
	case interface{ Unwrap() error }:
		return \"single:\" + x.Unwrap().Error()
	case interface{ Unwrap() []error }:
		return fmt.Sprint(\"multi:\", len(x.Unwrap()))
	default:
		return \"none\"
	}
}
func main() {
	fmt.Println(kind(one{}), kind(many{}), kind(leaf{}))
	var e error = leaf{}
	_, ok := e.(interface{ Unwrap() error })
	fmt.Println(ok)
}
";
    assert_stdout(src, "single:leaf multi:1 none\nfalse\n");
}

#[test]
fn fixed_width_integers_wrap_at_their_declared_width() {
    // Every sized type wraps at its own width, through ++, compound assignment,
    // binary operators and a function result; 64-bit types keep 64-bit wrapping.
    let src = "\
package main
import \"fmt\"
func fnv32(s string) uint32 {
	var h uint32 = 2166136261
	for i := 0; i < len(s); i++ {
		h ^= uint32(s[i])
		h *= 16777619
	}
	return h
}
func main() {
	var i8 int8 = 127
	i8++
	var u8 uint8 = 0
	u8--
	var i32 int32 = 2147483647
	i32++
	fmt.Println(i8, u8, i32)
	var n int = 127
	n++
	fmt.Println(n)
	var a int8 = 100
	fmt.Println(a+a, ^uint8(0))
	xs := []uint8{250}
	xs[0] += 10
	fmt.Println(xs[0])
	fmt.Println(fnv32(\"hello\"))
}
";
    assert_stdout(src, "-128 255 -2147483648\n128\n-56 255\n4\n1335831723\n");
}

#[test]
fn sync_waitgroup_mutex_and_once() {
    // The vendored `sync` over fusevm's cooperative scheduler: Wait parks until
    // every Done lands, a Mutex guards a shared counter, and Once runs once.
    let src = "\
package main
import (
	\"fmt\"
	\"sync\"
)
func main() {
	var wg sync.WaitGroup
	var mu sync.Mutex
	total := 0
	for i := 1; i <= 10; i++ {
		wg.Add(1)
		go func(n int) {
			defer wg.Done()
			mu.Lock()
			total += n
			mu.Unlock()
		}(i)
	}
	wg.Wait()
	var once sync.Once
	runs := 0
	for i := 0; i < 3; i++ {
		once.Do(func() { runs++ })
	}
	fmt.Println(total, runs, mu.TryLock())
}
";
    assert_stdout(src, "55 1 true\n");
}

#[test]
fn unsigned_64_bit_reads_the_sign_bit_unsigned() {
    // `uint64`/`uint`/`uintptr` share `int64`'s bit pattern, so `+ - * << & |`
    // need nothing — but `/`, `%`, `>>`, the ordered comparisons, the widening
    // to a float, and printing all consult the sign bit and must not read the
    // top-bit-set value as negative. Every number here was taken from `go run`.
    let src = "\
package main
import \"fmt\"
type box struct {
	u uint64
	n int
}
func half(x uint64) uint64 { return x / 2 }
func main() {
	var z uint64 = 0
	z--
	var u uint = 0
	u--
	fmt.Println(z, u)
	var x uint64 = 1 << 63
	fmt.Println(x, x > 100, x/3, x%7, x>>1)
	fmt.Printf(\"%d|%x|%o|%T\\n\", x, x, x, x)
	fmt.Println(half(x))
	fmt.Println(box{u: x, n: -1})
	fmt.Println([]uint64{x, 1}, map[string]uint64{\"a\": x})
	var c uint64 = 10
	c -= 20
	fmt.Println(c, c/3)
	fmt.Println(float64(x), int64(x))
	// A shift's type is the left operand's alone: a `uint` count must not turn
	// a signed shift into a logical one.
	var sh uint = 3
	var g int8 = -128
	fmt.Println(g >> sh)
}
";
    assert_stdout(
        src,
        "18446744073709551615 18446744073709551615\n\
         9223372036854775808 true 3074457345618258602 1 4611686018427387904\n\
         9223372036854775808|8000000000000000|1000000000000000000000|uint64\n\
         4611686018427387904\n\
         {9223372036854775808 -1}\n\
         [9223372036854775808 1] map[a:9223372036854775808]\n\
         18446744073709551606 6148914691236517202\n\
         9.223372036854776e+18 -9223372036854775808\n\
         -16\n",
    );
}

#[test]
fn recover_is_effective_only_in_the_directly_deferred_function() {
    // Go parks a propagating panic for the duration of each deferred call: the
    // deferred function runs normally and may call other functions before
    // recovering, but a `recover()` one frame deeper than the deferred call
    // returns nil. Both halves are load-bearing — treating "a panic is in
    // flight" as a post-call unwind trigger throws the deferred function out
    // before it reaches its own `recover()`.
    let src = "\
package main
import \"fmt\"
func helper() { fmt.Println(\"helper\") }
func indirect() (ok bool) {
	defer func() {
		helper()
		ok = recover() != nil
	}()
	panic(\"x\")
}
func doubled(v int) (r int) {
	defer func() { r *= 2 }()
	r = v
	return r
}
func main() {
	fmt.Println(indirect())
	fmt.Println(doubled(21))
	func() {
		defer func() {
			f := func() any { return recover() }
			fmt.Println(\"nested:\", f())
			fmt.Println(\"direct:\", recover())
		}()
		panic(\"p\")
	}()
	fmt.Println(\"bare:\", recover())
	func() {
		defer fmt.Println(\"d1\")
		defer fmt.Println(\"d2\")
		defer fmt.Println(\"d3\")
	}()
}
";
    assert_stdout(
        src,
        "helper\ntrue\n42\nnested: <nil>\ndirect: p\nbare: <nil>\nd3\nd2\nd1\n",
    );
}

#[test]
fn channel_receive_reports_closed_and_drained() {
    // `ok` is false exactly when the channel is closed AND drained — which a
    // channel delivering a real zero (`0`, `false`, `""`, a zero struct) must
    // not be confused with, and which is what ends a `range` over a channel.
    let src = "\
package main
import \"fmt\"
type pt struct{ x, y int }
func main() {
	z := make(chan int, 3)
	z <- 0
	z <- 0
	z <- 5
	close(z)
	sum, cnt := 0, 0
	for v := range z {
		sum += v
		cnt++
	}
	fmt.Println(sum, cnt)
	pc := make(chan pt, 1)
	pc <- pt{1, 2}
	close(pc)
	p1, ok1 := <-pc
	p2, ok2 := <-pc
	fmt.Println(p1, ok1, p2, ok2)
	bc := make(chan bool, 1)
	bc <- false
	close(bc)
	b1, k1 := <-bc
	b2, k2 := <-bc
	fmt.Println(b1, k1, b2, k2)
	// A closed channel makes its select case ready; comma-ok reports it, which
	// is how a select loop learns the channel is finished.
	d := make(chan int, 2)
	d <- 7
	d <- 8
	close(d)
	acc := 0
loop:
	for {
		select {
		case v, ok := <-d:
			if !ok {
				break loop
			}
			acc += v
		}
	}
	fmt.Println(\"acc\", acc)
	// A goroutine producer that closes ends the consumer's range.
	gc := make(chan int)
	go func() {
		for i := 1; i <= 4; i++ {
			gc <- i
		}
		close(gc)
	}()
	gs := 0
	for v := range gc {
		gs += v
	}
	fmt.Println(\"gs\", gs)
}
";
    assert_stdout(
        src,
        "5 3\n{1 2} true {0 0} false\nfalse true false false\nacc 15\ngs 10\n",
    );
}

#[test]
fn captured_variables_keep_their_declared_type_inside_a_closure() {
    // A lambda body is compiled with a fresh symbol table, so a captured
    // variable's declared type has to be carried in explicitly. Without it a
    // captured `chan int` is untyped inside the closure and `range` over it
    // lowers as a range over the channel handle's integer id — which silently
    // ran zero times instead of consuming the channel.
    let src = "\
package main
import (
	\"fmt\"
	\"sync\"
)
func main() {
	jobs := make(chan int, 8)
	out := make(chan int, 8)
	var wg sync.WaitGroup
	wg.Add(1)
	go func() {
		defer wg.Done()
		for j := range jobs {
			out <- j * j
		}
	}()
	for i := 1; i <= 4; i++ {
		jobs <- i
	}
	close(jobs)
	wg.Wait()
	close(out)
	total := 0
	for r := range out {
		total += r
	}
	fmt.Println(total)
	// A captured `uint64` likewise keeps reading unsigned inside the closure.
	var big uint64 = 1 << 63
	show := func() { fmt.Println(big, big/2, big > 1) }
	show()
}
";
    assert_stdout(src, "30\n9223372036854775808 4611686018427387904 true\n");
}

#[test]
fn comma_ok_receive_assigns_to_existing_variables() {
    // `v, ok = <-ch` (plain `=`) is the same lowering as the `:=` form but goes
    // through parallel assignment, which would otherwise reject it as an
    // arity mismatch. The element zero a closed receive yields must also be the
    // *typed* nil for a slice or map element, not an untyped nil.
    let src = "\
package main
import \"fmt\"
func main() {
	c := make(chan int, 1)
	c <- 5
	close(c)
	var v int
	var ok bool
	v, ok = <-c
	fmt.Println(v, ok)
	v, ok = <-c
	fmt.Println(v, ok)
	d := make(chan int, 1)
	d <- 3
	close(d)
	_, o := <-d
	x, _ := <-d
	fmt.Println(o, x)
	m := make(chan map[string]int, 1)
	close(m)
	mv, mok := <-m
	fmt.Println(mv, mok, mv == nil, len(mv))
	s := make(chan []int, 1)
	close(s)
	sv, sok := <-s
	fmt.Println(sv, sok, sv == nil, len(sv))
}
";
    assert_stdout(
        src,
        "5 true\n0 false\ntrue 0\nmap[] false true 0\n[] false true 0\n",
    );
}

#[test]
fn captured_narrow_widths_still_wrap_inside_a_closure() {
    // The widest blast radius of the fresh-symbol-table problem: without the
    // captured variable's declared type, `uint8` arithmetic inside a closure
    // stopped wrapping (250+10 printed 260) and `float32` stopped rounding to
    // 32 bits. Goroutine bodies and deferred closures are closures too, so this
    // reached most concurrent code.
    let src = "\
package main
import \"fmt\"
func main() {
	var b uint8 = 250
	h := func() uint8 { b += 10; return b }
	fmt.Println(h(), b)
	var i8 int8 = 120
	k := func() { i8 += 20; fmt.Println(i8, i8>>2, i8/3) }
	k()
	var u16 uint16 = 65530
	l := func() { u16 += 10; fmt.Println(u16) }
	l()
	var f float32 = 1.0 / 3.0
	g := func() { fmt.Println(f, f*3) }
	g()
	var u uint64 = 1 << 63
	outer := func() {
		inner := func() { fmt.Println(u, u/2, u > 1) }
		inner()
	}
	outer()
	done := make(chan uint8, 1)
	var gb uint8 = 200
	go func() { gb *= 2; done <- gb }()
	fmt.Println(<-done)
	func() {
		var db int8 = 100
		defer func() { db += 100; fmt.Println(\"deferred\", db) }()
	}()
}
";
    assert_stdout(
        src,
        "4 4\n-116 -29 -38\n4\n0.33333334 1\n\
         9223372036854775808 4611686018427387904 true\n144\ndeferred -56\n",
    );
}

/// Go copies a struct value transitively, but only through the fields that are
/// themselves struct *values*: a `*T`, slice or map field is a reference and the
/// copy shares it. Getting one half right and the other wrong is silent data
/// corruption in either direction, so both are pinned in one program — and each
/// container slot must get its own zero, or a write to one appears in all.
#[test]
fn struct_copy_is_transitive_but_stops_at_reference_fields() {
    let src = "package main

import \"fmt\"

type leaf struct{ N int }

type mid struct {
	L leaf
	N int
}

type rich struct {
	M mid
	P *leaf
	S []int
	H map[string]int
}

func (r rich) byValue()    { r.M.L.N = 90 }
func (r *rich) byPointer() { r.M.L.N = 70 }

func main() {
	base := rich{mid{leaf{1}, 2}, &leaf{3}, []int{4}, map[string]int{\"k\": 5}}
	cp := base
	cp.M.L.N = 10
	cp.P.N = 30
	cp.S[0] = 40
	cp.H[\"k\"] = 50
	fmt.Println(base.M.L.N, cp.M.L.N, base.P.N, base.S[0], base.H[\"k\"])

	base.byValue()
	fmt.Println(base.M.L.N)
	base.byPointer()
	fmt.Println(base.M.L.N)

	xs := []mid{base.M}
	out := xs[0]
	out.L.N = 11
	for _, v := range xs {
		v.L.N = 12
	}
	fmt.Println(xs[0].L.N, out.L.N)

	made := make([]mid, 3)
	made[0].L.N = 21
	fmt.Println(made)
}
";
    assert_stdout(
        src,
        "1 10 30 40 50\n1\n70\n70 11\n[{{21} 0} {{0} 0} {{0} 0}]\n",
    );
}

/// A fixed-size array is a Go *value*: `[N]T` is copied at assignment, argument
/// bind, return, container read and store, `append`, channel send and `range`,
/// and elementwise — so an array of arrays or of structs separates at every
/// depth. `[]T` is the same heap object here and must keep sharing at all of
/// them, so both halves are pinned in one program: getting either wrong is
/// silent data corruption in the opposite direction.
#[test]
fn array_copy_is_elementwise_but_its_slice_elements_stay_shared() {
    let src = "package main

import \"fmt\"

type pt struct{ X, Y int }

type grid struct {
	A [2]int
	Q [2]pt
	S []int
}

func bump(a [3]int, d int) [3]int {
	a[0] += d
	return a
}

func main() {
	a := [3]int{1, 2, 3}
	b := a
	b[0] = 9
	fmt.Println(a, b, bump(a, 5), a)

	n := [2][2]int{{1, 2}, {3, 4}}
	m := n
	m[0][0] = 8
	fmt.Println(n, m, n == m, [2]int{1, 2} == [2]int{1, 2})

	g := grid{A: [2]int{1, 2}, Q: [2]pt{{3, 4}, {5, 6}}, S: []int{7}}
	h := g
	h.A[1] = 20
	h.Q[1].Y = 60
	h.S[0] = 70
	fmt.Println(g.A, h.A, g.Q, h.Q, g.S, h.S)

	xs := [][2]int{{1, 2}, {3, 4}}
	r := xs[0]
	r[0] = 11
	q := [2]int{5, 6}
	xs[1] = q
	q[0] = 12
	fmt.Println(xs, r, q)

	zs := append([][2]int{}, xs...)
	zs[0][0] = 14
	fmt.Println(xs[0], zs[0])

	ch := make(chan [3]int, 1)
	ch <- a
	rv := <-ch
	rv[1] = 16
	fmt.Println(a, rv)

	sum := 0
	for i, ev := range a {
		if i == 0 {
			a[1] = 100
		}
		sum += ev
	}
	fmt.Println(sum, a)

	sh := [2][]int{{1, 2}, {3}}
	sk := sh
	sk[0][0] = 40
	sk[1] = []int{8}
	fmt.Println(sh, sk)

	var z [2][2]int
	var zg grid
	fmt.Println(z, zg)

	km := map[[2]int]string{{1, 2}: \"a\"}
	fmt.Println(km[[2]int{1, 2}], len(km))
}
";
    assert_stdout(
        src,
        "[1 2 3] [9 2 3] [6 2 3] [1 2 3]\n\
         [[1 2] [3 4]] [[8 2] [3 4]] false true\n\
         [1 2] [1 20] [{3 4} {5 6}] [{3 4} {5 60}] [70] [70]\n\
         [[1 2] [5 6]] [11 2] [12 6]\n\
         [1 2] [14 2]\n\
         [1 2 3] [1 16 3]\n\
         6 [1 100 3]\n\
         [[40 2] [3]] [[40 2] [8]]\n\
         [[0 0] [0 0]] {[0 0] [{0 0} {0 0}] []}\n\
         a 1\n",
    );
}

/// A verb applies to each element of a composite, and a `[]byte` is the text it
/// holds under `%s`/`%q`/`%x` — the two rules meet in the `[]byte`/`[]int` pair,
/// which hold identical values and must print differently.
#[test]
fn fmt_verbs_distribute_over_composites() {
    let src = "\
package main
import \"fmt\"
func main() {
	ws := []string{\"a\", \"b\"}
	is := []int{97, 98}
	bs := []byte(\"ab\")
	fmt.Printf(\"%q|%q|%q\\n\", ws, is, bs)
	fmt.Printf(\"%s|%s|%x\\n\", ws, bs, is)
	fmt.Printf(\"%q|%d\\n\", map[string]string{\"b\": \"y\", \"a\": \"x\"}, map[string]int{\"k\": 3})
	fmt.Printf(\"%f|%c|%U\\n\", []float64{1.5}, is, is)
	fmt.Printf(\"%8q|%.2q|%t\\n\", ws, []string{\"alpha\"}, is)
	fmt.Printf(\"%T|%T\\n\", bs, is)
}
";
    assert_stdout(
        src,
        "[\"a\" \"b\"]|['a' 'b']|\"ab\"\n\
         [a b]|ab|[61 62]\n\
         map[\"a\":\"x\" \"b\":\"y\"]|map[%!d(string=k):3]\n\
         [1.500000]|[a b]|[U+0061 U+0062]\n\
         [     \"a\"      \"b\"]|[\"al\"]|[%!t(int=97) %!t(int=98)]\n\
         []uint8|[]int\n",
    );
}

/// A negative operand under a base verb is a sign and a magnitude, not the
/// two's-complement bit pattern — the rule `%d` already followed.
#[test]
fn base_verbs_sign_a_negative_operand() {
    let src = "\
package main
import \"fmt\"
func main() {
	n := -9
	fmt.Printf(\"%x|%X|%o|%b\\n\", n, n, n, n)
	fmt.Printf(\"%#x|%#o|%#b|%#08x\\n\", n, n, n, n)
	var u uint64 = 18446744073709551615
	fmt.Printf(\"%x|%o\\n\", u, u)
}
";
    assert_stdout(
        src,
        "-9|-9|-11|-1001\n\
         -0x9|-011|-0b1001|-0x0000009\n\
         ffffffffffffffff|1777777777777777777777\n",
    );
}

/// A defined type is a distinct type carrying its base's representation: the
/// base's behaviour, plus a name that `%T` and `%#v` print and a method reaches
/// through.
#[test]
fn defined_types_keep_their_name_and_their_base() {
    let src = "\
package main
import \"fmt\"
type myInt int
type myStr string
type mySlice []int
type myMap map[string]int
func (m myInt) triple() myInt { return m * 3 }
func main() {
	n := myInt(7)
	fmt.Printf(\"%T %v %d %T\\n\", n, n, n+1, n.triple())
	var s myStr = \"hi\"
	fmt.Printf(\"%T %q %v\\n\", s, s, s+\"!\")
	sl := mySlice{3, 1}
	fmt.Printf(\"%T %v %#v %v\\n\", sl, sl, sl, len(sl))
	m := myMap{\"a\": 1}
	fmt.Printf(\"%T %v %d\\n\", m, m, m[\"a\"])
	var zs mySlice
	fmt.Printf(\"%T %v %v\\n\", zs, zs, int(n))
	fmt.Printf(\"%T\\n\", map[myStr]myInt{})
}
";
    assert_stdout(
        src,
        "main.myInt 7 8 main.myInt\n\
         main.myStr \"hi\" hi!\n\
         main.mySlice [3 1] main.mySlice{3, 1} 2\n\
         main.myMap map[a:1] 1\n\
         main.mySlice [] 7\n\
         map[main.myStr]main.myInt\n",
    );
}

/// Go refuses to build a program whose map key type is not comparable, and
/// names the type it refused. go-rs rejects the same four shapes at compile
/// time rather than building a map whose keys nothing can look up: the key is
/// hashed structurally, and a slice, map or func has no value to hash.
///
/// One of these — the slice key — is also a corpus file, which is what pins the
/// *exit status* against the reference. The rest are here because a corpus file
/// carries one program.
#[test]
fn a_map_key_type_go_rejects_is_a_compile_error() {
    // The third column is the type as the *parser* recorded it, which is what
    // the diagnostic names: a func type keeps only its `func` head there, so
    // `map[func()]int` is reported as `func`.
    for (key_ty, decl, named) in [
        ("[]int", "", "[]int"),
        ("map[string]int", "", "map[string]int"),
        ("func()", "", "func"),
        // A struct is comparable only when every field is.
        ("bad", "type bad struct { A int; B []int }\n", "bad"),
        // So is a defined type over one that is not.
        ("named", "type named []int\n", "named"),
    ] {
        let src = format!(
            "package main\nimport \"fmt\"\n{decl}func main() {{\n\tm := make(map[{key_ty}]int)\n\tfmt.Println(len(m))\n}}\n"
        );
        let (out, ok) = run_capturing_stderr(&src);
        assert!(!ok, "map[{key_ty}]int was accepted; output: {out:?}");
        assert!(
            out.contains(&format!("invalid map key type {named}")),
            "map[{key_ty}]int: {out:?}"
        );
    }
}

/// The types Go *does* accept as keys keep working — a pointer and a channel
/// are comparable by identity, an interface is comparable statically, and an
/// array or struct of comparable fields is comparable structurally. Without
/// this the check above is one over-eager predicate away from rejecting valid
/// programs.
#[test]
fn a_comparable_map_key_type_still_builds() {
    for (key_ty, decl) in [
        ("*int", ""),
        ("chan int", ""),
        ("interface{}", ""),
        ("[2]int", ""),
        ("string", ""),
        ("ok", "type ok struct { A int; B string }\n"),
        ("alias", "type alias int\n"),
        ("arr", "type arr [2]string\n"),
    ] {
        let src = format!(
            "package main\nimport \"fmt\"\n{decl}func main() {{\n\tm := make(map[{key_ty}]int)\n\tfmt.Println(len(m))\n}}\n"
        );
        let (out, ok) = run_capturing_stderr(&src);
        assert!(ok, "map[{key_ty}]int was rejected; output: {out:?}");
        assert_eq!(out, "0\n", "map[{key_ty}]int");
    }
}
