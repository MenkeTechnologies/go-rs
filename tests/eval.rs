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
