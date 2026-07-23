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
    let src = "\
package main
import (
	\"fmt\"
	\"strconv\"
)
func main() {
	fmt.Println(strconv.Itoa(42), strconv.Atoi(\"100\")+1)
}
";
    assert_stdout(src, "42 101\n");
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
    // A full slice expression s[low:high:max] — the capacity bound is accepted
    // (and ignored, since go-rs sub-slices copy).
    let src = "\
package main
import \"fmt\"
func main() {
	s := []int{1, 2, 3, 4, 5}
	fmt.Println(s[1:3:4], len(s[1:3:4]))
	fmt.Println(s[:2:5])
}
";
    assert_stdout(src, "[2 3] 2\n[1 2]\n");
}
