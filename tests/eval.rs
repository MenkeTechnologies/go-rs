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
