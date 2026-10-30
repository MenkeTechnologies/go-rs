//! Regressions for the fifth round of differential testing against the
//! reference `go`: what an unrecovered panic, a run-time fault and a deadlock
//! print on stderr, and the exit status they leave. Each expected text
//! is the reference's own output for the same program — stdout verbatim, and
//! stderr up to the goroutine dump, which go-rs does not reproduce — so the tests need no `go`
//! toolchain.

use std::process::Command;

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

/// A deadlock is Go's runtime fatal error on stderr with exit status 2, not a
/// toolchain diagnostic; stdout written before it is kept.
#[test]
fn deadlock_unbuffered_send() {
    let (out, err, code) = run_src(
        r#"package main

import "fmt"

func main() {
	fmt.Println("before")
	ch := make(chan int)
	ch <- 1
}
"#,
    );
    assert_eq!(
        out,
        r#"before
"#
    );
    assert_eq!(
        err,
        r#"fatal error: all goroutines are asleep - deadlock!
"#
    );
    assert_eq!(code, 2);
}

/// A WaitGroup that never reaches zero with nothing left to run is the same
/// fatal deadlock.
#[test]
fn deadlock_waitgroup() {
    let (out, err, code) = run_src(
        r#"package main

import "sync"

func main() {
	var wg sync.WaitGroup
	wg.Add(1)
	wg.Wait()
}
"#,
    );
    assert_eq!(out, "");
    assert_eq!(
        err,
        r#"fatal error: all goroutines are asleep - deadlock!
"#
    );
    assert_eq!(code, 2);
}

/// A deferred call that recovers a panic and then panics prints the recovered
/// one first, marked `[recovered]`, and the new one beneath it.
#[test]
fn repanic_after_recover() {
    let (out, err, code) = run_src(
        r#"package main

import "fmt"

func main() {
	defer func() {
		r := recover()
		fmt.Println("got", r)
		panic("again")
	}()
	panic("first")
}
"#,
    );
    assert_eq!(
        out,
        r#"got first
"#
    );
    assert_eq!(
        err,
        r#"panic: first [recovered]
	panic: again
"#
    );
    assert_eq!(code, 2);
}

/// A deferred call that panics while a panic propagates prints both, the
/// superseded one first.
#[test]
fn panic_in_deferred_call() {
    let (out, err, code) = run_src(
        r#"package main

import "fmt"

func main() {
	defer fmt.Println("d1")
	defer func() {
		panic("second")
	}()
	panic("first")
}
"#,
    );
    assert_eq!(
        out,
        r#"d1
"#
    );
    assert_eq!(
        err,
        r#"panic: first
	panic: second
"#
    );
    assert_eq!(code, 2);
}

/// `panic(recover())` re-raises the same value: Go prints it once, marked
/// `[recovered, repanicked]`.
#[test]
fn repanic_same_value() {
    let (out, err, code) = run_src(
        r#"package main

func main() {
	defer func() {
		r := recover()
		panic(r)
	}()
	panic("x")
}
"#,
    );
    assert_eq!(out, "");
    assert_eq!(
        err,
        r#"panic: x [recovered, repanicked]
"#
    );
    assert_eq!(code, 2);
}

/// A panic recovered by a call that then returned normally is gone: it is not
/// printed above a later fatal one.
#[test]
fn recovered_panic_not_reprinted() {
    let (out, err, code) = run_src(
        r#"package main

import "fmt"

func f() {
	defer func() { fmt.Println("rec", recover()) }()
	panic("one")
}

func main() {
	f()
	defer func() { panic("three") }()
	panic("two")
}
"#,
    );
    assert_eq!(
        out,
        r#"rec one
"#
    );
    assert_eq!(
        err,
        r#"panic: two
	panic: three
"#
    );
    assert_eq!(code, 2);
}

/// Every link of the chain prints through `Error()` / `String()`.
#[test]
fn panic_chain_error_and_stringer() {
    let (out, err, code) = run_src(
        r#"package main

import (
	"errors"
	"fmt"
)

type St int

func (s St) String() string { return fmt.Sprint("st", int(s)) }

func main() {
	defer func() {
		recover()
		panic(errors.New("e3"))
	}()
	defer func() { panic(St(2)) }()
	panic(St(1))
}
"#,
    );
    assert_eq!(out, "");
    assert_eq!(
        err,
        r#"panic: st1
	panic: st2 [recovered]
	panic: e3
"#
    );
    assert_eq!(code, 2);
}

/// A recovered panic stays on the chain while its deferred call runs, so a
/// run-time error raised there prints beneath it.
#[test]
fn panic_chain_runtime_error() {
    let (out, err, code) = run_src(
        r#"package main

import "fmt"

func inner() {
	defer func() {
		fmt.Println("inner rec", recover())
	}()
	defer func() { panic("b") }()
	panic("a")
}

func main() {
	inner()
	defer func() {
		r := recover()
		fmt.Println("main rec", r)
		var s []int
		_ = s[r.(int)]
	}()
	panic(7)
}
"#,
    );
    assert_eq!(
        out,
        r#"inner rec b
main rec 7
"#
    );
    assert_eq!(
        err,
        r#"panic: 7 [recovered]
	panic: runtime error: index out of range [7] with length 0
"#
    );
    assert_eq!(code, 2);
}

/// `wg.Done()` past zero panics with Go's `sync: negative WaitGroup counter`.
#[test]
fn negative_waitgroup_counter() {
    let (out, err, code) = run_src(
        r#"package main

import "sync"

func main() {
	var wg sync.WaitGroup
	wg.Done()
}
"#,
    );
    assert_eq!(out, "");
    assert_eq!(
        err,
        r#"panic: sync: negative WaitGroup counter
"#
    );
    assert_eq!(code, 2);
}

/// `sync.OnceFunc`: a panicking function panics with the same value on every
/// call of the wrapper, and runs only once.
#[test]
fn oncefunc_panic() {
    let (out, err, code) = run_src(
        r#"package main

import (
	"fmt"
	"sync"
)

func main() {
	g := sync.OnceFunc(func() { panic("boom") })
	for i := 0; i < 2; i++ {
		func() {
			defer func() { fmt.Println("r", recover()) }()
			g()
		}()
	}
	g()
}
"#,
    );
    assert_eq!(
        out,
        r#"r boom
r boom
"#
    );
    assert_eq!(
        err,
        r#"panic: boom
"#
    );
    assert_eq!(code, 2);
}

/// Indexing through a nil pointer to an array is Go's nil-dereference run-time
/// panic, not a go-rs diagnostic.
#[test]
fn nil_array_pointer_index() {
    let (out, err, code) = run_src(
        r#"package main

import "fmt"

func main() {
	var p *[3]int
	fmt.Println(p[1])
}
"#,
    );
    assert_eq!(out, "");
    assert_eq!(
        err,
        r#"panic: runtime error: invalid memory address or nil pointer dereference
"#
    );
    assert_eq!(code, 2);
}

/// Storing through a nil pointer to an array is the same panic, and `recover()`
/// yields it.
#[test]
fn nil_array_pointer_store() {
    let (out, err, code) = run_src(
        r#"package main

import "fmt"

func main() {
	defer func() { fmt.Println(recover()) }()
	var p *[3]int
	p[1] = 2
}
"#,
    );
    assert_eq!(
        out,
        r#"runtime error: invalid memory address or nil pointer dereference
"#
    );
    assert_eq!(err, "");
    assert_eq!(code, 0);
}
