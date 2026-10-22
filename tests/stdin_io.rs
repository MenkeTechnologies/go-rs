//! Standard input through `os.Stdin`: `bufio.Scanner`, `bufio.Reader`,
//! `io.ReadAll` and a bare `os.Stdin.Read`, each run through the built `go`
//! binary with the input piped in — the one path the byte-parity corpus cannot
//! take, because its runner feeds every program the same (empty) stdin.
//!
//! Every expected string here is what the reference `go run` prints for the
//! same program and input.

use std::io::Write;
use std::process::{Command, Stdio};

/// Compile and run `src` with `stdin` piped to it; return (stdout, success).
fn run_with_stdin(src: &str, stdin: &[u8]) -> (String, bool) {
    run_with_stdin_env(src, stdin, &[])
}

/// As [`run_with_stdin`], with extra environment variables for the child.
fn run_with_stdin_env(src: &str, stdin: &[u8], env: &[(&str, &str)]) -> (String, bool) {
    let mut f = tempfile::Builder::new()
        .suffix(".go")
        .tempfile()
        .expect("temp file");
    f.write_all(src.as_bytes()).expect("write source");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_go"));
    cmd.arg("run")
        .arg(f.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().expect("spawn go binary");
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(stdin)
        .expect("write stdin");
    let out = child.wait_with_output().expect("collect child output");
    (
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr),
        out.status.success(),
    )
}

fn assert_stdout(src: &str, stdin: &[u8], expected: &str) {
    let (out, ok) = run_with_stdin(src, stdin);
    assert!(ok, "program failed; output was: {out:?}");
    assert_eq!(out, expected);
}

#[test]
fn scanner_reads_stdin_line_by_line() {
    let src = r#"package main

import (
	"bufio"
	"fmt"
	"os"
	"strings"
)

func main() {
	sc := bufio.NewScanner(os.Stdin)
	n := 0
	for sc.Scan() {
		line := sc.Text()
		n++
		fmt.Println(n, strings.ToUpper(line), len(line))
	}
	fmt.Println("err:", sc.Err())
}
"#;
    // A multibyte line, a tab-indented one, and a last line with no newline —
    // which the Scanner still yields.
    assert_stdout(
        src,
        b"hello w\xc3\xb6rld  foo\n\tbar baz\nlast",
        "1 HELLO WÖRLD  FOO 17\n2 \tBAR BAZ 8\n3 LAST 4\nerr: <nil>\n",
    );
}

#[test]
fn scanner_on_empty_stdin_ends_without_error() {
    let src = r#"package main

import (
	"bufio"
	"fmt"
	"os"
)

func main() {
	sc := bufio.NewScanner(os.Stdin)
	for sc.Scan() {
		fmt.Println("unexpected token", sc.Text())
	}
	fmt.Println("err:", sc.Err())
}
"#;
    assert_stdout(src, b"", "err: <nil>\n");
}

#[test]
fn scanner_split_words_and_bytes() {
    let src = r#"package main

import (
	"bufio"
	"fmt"
	"os"
)

func main() {
	sc := bufio.NewScanner(os.Stdin)
	sc.Split(bufio.ScanWords)
	n, total := 0, 0
	for sc.Scan() {
		n++
		total += len(sc.Bytes())
		fmt.Printf("%d:%q\n", n, sc.Text())
	}
	fmt.Println(n, total, sc.Err())
}
"#;
    assert_stdout(
        src,
        b" the\tquick  brown\nfox \n",
        "1:\"the\"\n2:\"quick\"\n3:\"brown\"\n4:\"fox\"\n4 16 <nil>\n",
    );
}

#[test]
fn scanner_token_past_max_size_is_err_too_long_unless_buffered() {
    let src = r#"package main

import (
	"bufio"
	"fmt"
	"os"
)

func main() {
	sc := bufio.NewScanner(os.Stdin)
	for sc.Scan() {
		fmt.Println("tok", len(sc.Text()))
	}
	fmt.Println(sc.Err(), sc.Err() == bufio.ErrTooLong)
}
"#;
    let mut input = vec![b'x'; bufio_max_scan_token_size() + 1];
    input.extend_from_slice(b"\nshort\n");
    assert_stdout(src, &input, "bufio.Scanner: token too long true\n");

    let src_buffered = r#"package main

import (
	"bufio"
	"fmt"
	"os"
)

func main() {
	sc := bufio.NewScanner(os.Stdin)
	sc.Buffer(make([]byte, 100), 1<<20)
	for sc.Scan() {
		fmt.Println("tok", len(sc.Text()))
	}
	fmt.Println(sc.Err())
}
"#;
    assert_stdout(src_buffered, &input, "tok 65537\ntok 5\n<nil>\n");
}

/// Go's `bufio.MaxScanTokenSize`, pinned here so the oversized input above is
/// built from the same number the vendored `scan.go` declares.
fn bufio_max_scan_token_size() -> usize {
    64 * 1024
}

#[test]
fn reader_read_string_reports_eof_on_unterminated_last_line() {
    let src = r#"package main

import (
	"bufio"
	"fmt"
	"io"
	"os"
)

func main() {
	r := bufio.NewReader(os.Stdin)
	for {
		line, err := r.ReadString('\n')
		fmt.Printf("%q %v\n", line, err)
		if err == io.EOF {
			break
		}
	}
	b, err := r.ReadByte()
	fmt.Println(b, err)
}
"#;
    assert_stdout(
        src,
        b"line one\nline two\nno newline",
        "\"line one\\n\" <nil>\n\"line two\\n\" <nil>\n\"no newline\" EOF\n0 EOF\n",
    );
}

#[test]
fn reader_rune_peek_line_and_discard() {
    let src = r#"package main

import (
	"bufio"
	"fmt"
	"io"
	"os"
)

func main() {
	r := bufio.NewReader(os.Stdin)
	rn, size, err := r.ReadRune()
	fmt.Println(string(rn), size, err, r.UnreadRune())
	p, err := r.Peek(3)
	fmt.Printf("%q %v\n", p, err)
	line, isPrefix, err := r.ReadLine()
	fmt.Printf("%q %v %v\n", line, isPrefix, err)
	bs, err := r.ReadBytes('\n')
	fmt.Printf("%q %v\n", bs, err)
	d, err := r.Discard(2)
	fmt.Println(d, err, r.Buffered())
	rest, err := io.ReadAll(r)
	fmt.Printf("%q %v\n", rest, err)
	_, err = r.ReadByte()
	fmt.Println(err == io.EOF)
}
"#;
    assert_stdout(
        src,
        "héllo\nwörld\nxyzzy\nfinal".as_bytes(),
        "h 1 <nil> <nil>\n\"hé\" <nil>\n\"héllo\" false <nil>\n\"wörld\\n\" <nil>\n2 <nil> 9\n\"zzy\\nfinal\" <nil>\ntrue\n",
    );
}

#[test]
fn io_read_all_drains_stdin_and_a_later_read_is_eof() {
    let src = r#"package main

import (
	"fmt"
	"io"
	"os"
)

func main() {
	data, err := io.ReadAll(os.Stdin)
	fmt.Println(len(data), err, string(data[:5]))
	buf := make([]byte, 4)
	n, err := os.Stdin.Read(buf)
	fmt.Println(n, err, err == io.EOF)
	fmt.Println(os.Stdin.Fd())
}
"#;
    // Larger than io.ReadAll's first 512-byte buffer, so it grows across reads.
    let input: Vec<u8> = (0..700u32).map(|i| b'a' + (i % 26) as u8).collect();
    assert_stdout(src, &input, "700 <nil> abcde\n0 EOF true\n0\n");
}

#[test]
fn stdin_read_returns_short_reads_as_is() {
    // `os.Stdin.Read` fills what it can of the caller's buffer and reports the
    // count; a buffer larger than the input returns the whole input in one
    // call and EOF on the next, without padding.
    let src = r#"package main

import (
	"fmt"
	"os"
)

func main() {
	buf := make([]byte, 16)
	n, err := os.Stdin.Read(buf)
	fmt.Println(n, err, string(buf[:n]))
	n, err = os.Stdin.Read(buf)
	fmt.Println(n, err)
	n, err = os.Stdin.Read(buf[:0])
	fmt.Println(n, err)
}
"#;
    assert_stdout(src, b"abc", "3 <nil> abc\n0 EOF\n0 <nil>\n");
}

#[test]
fn call_through_field_of_defined_func_type() {
    // `bufio.Scanner` calls its `split SplitFunc` field — a field whose type is
    // a *defined* func type. The call must read the field and dispatch its
    // value, not look for a method named `split`.
    let src = r#"package main

import "fmt"

type Op func(a, b int) (int, bool)

type calc struct {
	name string
	op   Op
}

func main() {
	c := calc{"add", func(a, b int) (int, bool) { return a + b, a > b }}
	v, ok := c.op(2, 3)
	fmt.Println(c.name, v, ok)
	p := &c
	p.op = func(a, b int) (int, bool) { return a * b, true }
	v, ok = p.op(4, 5)
	fmt.Println(v, ok)
}
"#;
    assert_stdout(src, b"", "add 5 false\n20 true\n");
}

#[test]
fn bufio_io_and_utf8_resolve_without_a_go_toolchain() {
    // The read half of bufio and `unicode/utf8` are vendored into the binary:
    // with no `$GOROOT` and no `~/.go-rs/src` to fall back on, the program
    // still links and runs.
    let src = r#"package main

import (
	"bufio"
	"fmt"
	"os"
	"unicode/utf8"
)

func main() {
	sc := bufio.NewScanner(os.Stdin)
	for sc.Scan() {
		fmt.Println(utf8.RuneCountInString(sc.Text()), len(sc.Text()), utf8.ValidString(sc.Text()))
	}
}
"#;
    let (out, ok) = run_with_stdin_env(
        src,
        "日本語\nabc\n".as_bytes(),
        &[("GOROOT", "/nonexistent"), ("GO_RS_HOME", "/nonexistent")],
    );
    assert!(ok, "program failed; output was: {out:?}");
    assert_eq!(out, "3 9 true\n3 3 true\n");
}
