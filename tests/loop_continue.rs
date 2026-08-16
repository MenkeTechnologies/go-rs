//! `continue` in every loop form, under a wall-clock bound.
//!
//! The failure this guards against is not a wrong answer but a hang: a
//! `continue` whose jump target is patched to 0 re-enters the loop body at the
//! top of the *chunk* instead of at the post statement, and the program spins
//! forever. That bug has been found in this frontend family before (a `Jump(0)`
//! in the for-loop lowering), and it is invisible to a differential probe
//! corpus — a process that never exits produces no output to diff against `go`,
//! so the harness times out and reports the case as "no output" rather than as
//! the infinite loop it is.
//!
//! So these tests run the child with a deadline and turn a hang into a failed
//! assertion. The expected values are the verbatim stdout of `go run` on the
//! same source (checked against `go1.26.6 darwin/arm64`).

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long a loop test may run before it is treated as non-terminating. Every
/// program here does a few dozen iterations and exits in milliseconds; the
/// margin is for a loaded CI box, not for slow work.
const DEADLINE: Duration = Duration::from_secs(20);

/// Run `src` through the built `go` binary, killing it at [`DEADLINE`].
///
/// Returns `None` when the deadline was hit, which is the signal a caller turns
/// into "this loop did not terminate" — distinct from a program that ran and
/// printed the wrong thing.
fn run_bounded(src: &str) -> Option<(String, bool)> {
    let mut f = tempfile::Builder::new()
        .suffix(".go")
        .tempfile()
        .expect("temp file");
    f.write_all(src.as_bytes()).expect("write source");

    let mut child = Command::new(env!("CARGO_BIN_EXE_go"))
        .arg("run")
        .arg(f.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn go binary");

    let start = Instant::now();
    loop {
        match child.try_wait().expect("poll child") {
            Some(_) => break,
            None if start.elapsed() >= DEADLINE => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            // Poll rather than block, so the deadline is enforced even when the
            // child never writes anything and never exits.
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    let out = child.wait_with_output().expect("collect child output");
    Some((
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.success(),
    ))
}

/// Assert `src` terminates inside the deadline and prints exactly `expected`.
fn assert_terminates_with(src: &str, expected: &str) {
    let Some((stdout, ok)) = run_bounded(src) else {
        panic!("program did not terminate within {DEADLINE:?} — a `continue` jump target is wrong");
    };
    assert!(ok, "program failed; stdout was: {stdout:?}");
    assert_eq!(stdout, expected);
}

/// The three `for` forms. In each, `continue` must reach the post statement (or
/// the condition), never the top of the body — skipping the `i++` in the
/// three-clause form is the shape that spins forever.
#[test]
fn continue_advances_every_for_form() {
    let src = r#"package main

import "fmt"

func main() {
	s := 0
	for i := 0; i < 6; i++ {
		if i%2 == 0 {
			continue
		}
		s += i
	}
	fmt.Println("three-clause", s)

	j, t := 0, 0
	for j < 6 {
		j++
		if j%2 == 0 {
			continue
		}
		t += j
	}
	fmt.Println("cond-only", t)

	k, u := 0, 0
	for {
		k++
		if k > 6 {
			break
		}
		if k%2 == 0 {
			continue
		}
		u += k
	}
	fmt.Println("bare", u)
}
"#;
    assert_terminates_with(src, "three-clause 9\ncond-only 9\nbare 9\n");
}

/// `continue` inside every `range` form, including the integer range Go 1.22
/// added. A range loop advances its own cursor, so a mis-targeted jump here
/// re-runs the same element rather than the same iteration.
#[test]
fn continue_advances_every_range_form() {
    let src = r#"package main

import "fmt"

func main() {
	r := 0
	for _, v := range []int{1, 2, 3, 4, 5} {
		if v%2 == 0 {
			continue
		}
		r += v
	}
	fmt.Println("slice", r)

	m := map[string]int{"a": 1, "b": 2, "c": 3}
	mm := 0
	for _, v := range m {
		if v == 2 {
			continue
		}
		mm += v
	}
	fmt.Println("map", mm)

	rs := 0
	for _, c := range "abcde" {
		if c == 'c' {
			continue
		}
		rs += int(c)
	}
	fmt.Println("string", rs)

	ri := 0
	for i := range 6 {
		if i%2 == 0 {
			continue
		}
		ri += i
	}
	fmt.Println("int", ri)
}
"#;
    assert_terminates_with(src, "slice 9\nmap 4\nstring 396\nint 9\n");
}

/// A `continue` that is the last statement of the body has no code after it to
/// jump over, which is exactly the case a lowering can leave with a zero
/// operand and never notice.
#[test]
fn a_trailing_continue_still_advances() {
    let src = r#"package main

import "fmt"

func main() {
	n := 0
	for i := 0; i < 5; i++ {
		n += i
		continue
	}
	fmt.Println(n)

	e := 0
	for i := 0; i < 5; i++ {
		if i > 90 {
			continue
		}
		e++
	}
	fmt.Println(e)
}
"#;
    assert_terminates_with(src, "10\n5\n");
}

/// `continue` from inside a `switch` and a `select` binds to the enclosing loop,
/// not to the statement it sits in — `break` is the one that binds inward.
#[test]
fn continue_from_switch_and_select_binds_to_the_loop() {
    let src = r#"package main

import "fmt"

func main() {
	sw := 0
	for i := 0; i < 6; i++ {
		switch i % 3 {
		case 0:
			continue
		case 1:
			sw += i
		default:
			sw += 100
		}
	}
	fmt.Println("switch", sw)

	ch := make(chan int, 3)
	ch <- 1
	ch <- 2
	ch <- 3
	sel := 0
	for i := 0; i < 3; i++ {
		select {
		case v := <-ch:
			if v == 2 {
				continue
			}
			sel += v
		}
	}
	fmt.Println("select", sel)
}
"#;
    assert_terminates_with(src, "switch 205\nselect 4\n");
}

/// A labeled `continue` re-enters the *named* loop, running its post statement —
/// so the outer counter advances and the nest terminates.
#[test]
fn a_labeled_continue_advances_the_named_loop() {
    let src = r#"package main

import "fmt"

func main() {
	n := 0
outer:
	for i := 0; i < 4; i++ {
		for j := 0; j < 4; j++ {
			if j > i {
				continue outer
			}
			n++
		}
	}
	fmt.Println(n)

	rows := 0
mid:
	for _, r := range [][]int{{1, 2}, {3}, {4, 5}} {
		for _, v := range r {
			if v == 3 {
				continue mid
			}
			rows += v
		}
	}
	fmt.Println(rows)
}
"#;
    assert_terminates_with(src, "10\n12\n");
}
