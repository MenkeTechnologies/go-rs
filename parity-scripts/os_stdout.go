package main

// `os.Stdout` / `os.Stderr` — the two files a Go program writes to without
// opening anything, and the reason `fmt.Fprintln(os.Stderr, …)` is the idiom
// for a diagnostic.
//
// `os` is a native host package, so these arrive the way `strings.Builder`
// does: synthesized package-level vars over a `*os.File` whose `Write` reaches
// a host intrinsic. Only stdout is compared here — the harness diffs stdout —
// so the stderr half is exercised by writing to it and checking that what
// lands on *stdout* is exactly what was sent there.

import (
	"fmt"
	"io"
	"os"
)

func write(w io.Writer, s string) { fmt.Fprint(w, s) }

func main() {
	fmt.Fprintln(os.Stdout, "fprintln")
	fmt.Fprintf(os.Stdout, "%s=%d\n", "k", 1)
	fmt.Fprint(os.Stdout, "fprint", 2, "\n")

	n, err := os.Stdout.Write([]byte("write\n"))
	m, err2 := os.Stdout.WriteString("writestring\n")
	fmt.Println("counts", n, err, m, err2)

	w, err3 := io.WriteString(os.Stdout, "iowritestring\n")
	fmt.Println("io", w, err3)

	// Held in an interface variable and passed down.
	var iw io.Writer = os.Stdout
	fmt.Fprint(iw, "via-iface\n")
	write(os.Stdout, "via-param\n")

	// The descriptors are the conventional ones, and the two files differ.
	fmt.Println("fds", os.Stdout.Fd(), os.Stderr.Fd())

	// Everything written to stderr stays off stdout.
	fmt.Fprintln(os.Stderr, "this must not appear on stdout")
	os.Stderr.WriteString("nor this\n")

	// Interleaving with fmt's own stdout writes keeps program order.
	fmt.Print("a")
	fmt.Fprint(os.Stdout, "b")
	fmt.Print("c\n")

	// os.Stdout is one value, not a fresh one per mention.
	x := os.Stdout
	y := os.Stdout
	fmt.Println("same", x == y, x.Fd() == y.Fd())
}
