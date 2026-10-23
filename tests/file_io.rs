//! Files on disk: `os.ReadFile` / `WriteFile` / `Create` / `OpenFile` /
//! `Remove` and the `*os.File` they hand back, each program run through the
//! built `go` binary against a fresh temporary directory passed as `os.Args[1]`.
//! The byte-parity corpus covers only the paths whose outcome is the same on
//! every machine (`parity-scripts/os_file_errors.go`); anything that creates a
//! file lives here, so concurrent corpus runs never share a scratch path.
//!
//! Every expected string here is what the reference `go run` prints for the
//! same program.

use std::io::Write;
use std::process::Command;

/// Compile and run `src` with a new temporary directory as its one argument;
/// return (stdout + stderr, success).
fn run_in_temp_dir(src: &str) -> (String, bool) {
    let mut f = tempfile::Builder::new()
        .suffix(".go")
        .tempfile()
        .expect("temp file");
    f.write_all(src.as_bytes()).expect("write source");
    let dir = tempfile::tempdir().expect("temp dir");
    let out = Command::new(env!("CARGO_BIN_EXE_go"))
        .arg("run")
        .arg(f.path())
        .arg(dir.path())
        // No `$GOROOT` and no `~/.go-rs/src`: `os` and the `io`, `io/fs`,
        // `syscall` and `internal/oserror` sources it links must all resolve
        // to the copies vendored into the binary.
        .env("GOROOT", "/nonexistent")
        .env("GO_RS_HOME", "/nonexistent")
        .output()
        .expect("run go binary");
    (
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr),
        out.status.success(),
    )
}

fn assert_stdout(src: &str, expected: &str) {
    let (out, ok) = run_in_temp_dir(src);
    assert!(ok, "program failed; output was: {out:?}");
    assert_eq!(out, expected);
}

/// `WriteFile` then `ReadFile`: text, a file past the first 512-byte buffer
/// (whose capacity is Go's stat size + 1), and every byte value 0..=255.
#[test]
fn write_file_then_read_file_round_trips_bytes() {
    let src = r#"
package main

import (
	"fmt"
	"os"
)

func main() {
	dir := os.Args[1]
	name := dir + "/notes.txt"
	fmt.Println(os.WriteFile(name, []byte("alpha\nbeta\n"), 0644))
	data, err := os.ReadFile(name)
	fmt.Printf("%q %v %d %d\n", data, err, len(data), cap(data))

	big := make([]byte, 1000)
	for i := range big {
		big[i] = byte('a' + i%26)
	}
	fmt.Println(os.WriteFile(name, big, 0644))
	data, err = os.ReadFile(name)
	fmt.Println(len(data), cap(data), err, string(data[995:]))

	bin := make([]byte, 256)
	for i := range bin {
		bin[i] = byte(i)
	}
	fmt.Println(os.WriteFile(name, bin, 0600))
	data, _ = os.ReadFile(name)
	sum := 0
	for _, b := range data {
		sum += int(b)
	}
	fmt.Println(len(data), sum, data[0], data[127], data[128], data[255])
}
"#;
    assert_stdout(
        src,
        "<nil>\n\"alpha\\nbeta\\n\" <nil> 11 512\n<nil>\n1000 1001 <nil> hijkl\n<nil>\n256 32640 0 127 128 255\n",
    );
}

/// `Create`, writes through `WriteString` / `Fprintf` / a `bufio.Writer`,
/// an `O_APPEND` reopen, a `bufio.Scanner` over `Open`, `O_EXCL` on an existing
/// file (`file exists`, `os.IsExist`) and `Create` truncating.
#[test]
fn create_append_scan_and_exclusive_open() {
    let src = r#"
package main

import (
	"bufio"
	"fmt"
	"os"
)

func main() {
	name := os.Args[1] + "/log.txt"
	f, err := os.Create(name)
	if err != nil {
		fmt.Println(err)
		return
	}
	fmt.Println(f.Name() == name)
	n, err := f.WriteString("one\n")
	fmt.Println(n, err)
	n, err = fmt.Fprintf(f, "two %d\n", 2)
	fmt.Println(n, err)
	w := bufio.NewWriter(f)
	fmt.Fprintln(w, "three")
	fmt.Println(w.Flush(), f.Close())

	a, err := os.OpenFile(name, os.O_APPEND|os.O_WRONLY, 0)
	fmt.Println(err)
	a.Write([]byte("four\n"))
	a.Close()

	r, err := os.Open(name)
	fmt.Println(err)
	defer r.Close()
	sc := bufio.NewScanner(r)
	for sc.Scan() {
		fmt.Printf("[%s]\n", sc.Text())
	}
	fmt.Println(sc.Err())

	_, err = os.OpenFile(name, os.O_RDWR|os.O_CREATE|os.O_EXCL, 0644)
	fmt.Println(err == nil, os.IsExist(err))
	fmt.Println(err.Error() == "open "+name+": file exists")

	f, _ = os.Create(name)
	f.Close()
	data, _ := os.ReadFile(name)
	fmt.Println(len(data))
}
"#;
    assert_stdout(
        src,
        "true\n4 <nil>\n6 <nil>\n<nil> <nil>\n<nil>\n<nil>\n[one]\n[two 2]\n[three]\n[four]\n<nil>\nfalse true\ntrue\n0\n",
    );
}

/// `Remove` of a file, of a missing file (`ENOENT`), of a non-empty directory
/// (`ENOTEMPTY`, which `syscall.Errno.Is` maps to `fs.ErrExist`) and of the
/// emptied directory.
#[test]
fn remove_files_and_directories() {
    let src = r#"
package main

import (
	"errors"
	"fmt"
	"io/fs"
	"os"
	"syscall"
)

func main() {
	dir := os.Args[1]
	file := dir + "/f.txt"
	os.WriteFile(file, []byte("x"), 0644)
	fmt.Println(os.Remove(file))
	_, err := os.ReadFile(file)
	fmt.Println(os.IsNotExist(err))
	err = os.Remove(file)
	fmt.Println(os.IsNotExist(err), err.Error() == "remove "+file+": no such file or directory")

	os.WriteFile(file, []byte("x"), 0644)
	err = os.Remove(dir)
	var pe *fs.PathError
	fmt.Println(errors.As(err, &pe), pe.Op, pe.Path == dir)
	fmt.Println(pe.Err == syscall.ENOTEMPTY, pe.Err, errors.Is(err, fs.ErrExist))
	fmt.Println(os.Remove(file), os.Remove(dir))
	_, err = os.Open(dir)
	fmt.Println(os.IsNotExist(err))
}
"#;
    assert_stdout(
        src,
        "<nil>\ntrue\ntrue true\ntrue remove true\ntrue directory not empty true\n<nil> <nil>\ntrue\n",
    );
}
