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
