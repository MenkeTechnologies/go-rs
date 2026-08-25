package main

// `strings.Builder` — a growable string, and an `io.Writer`. It cannot arrive
// through the source-package path the way `errors` and `io` do, because
// `strings` is a native host package whose functions are builtins; the type is
// synthesized and qualified instead, which is why its whole method set is worth
// pinning against the reference rather than assuming.
//
// `Cap` is deliberately absent — go-rs cannot reserve capacity, so a program
// that calls it gets a compile error rather than a confident wrong number.

import (
	"fmt"
	"io"
	"strings"
)

func main() {
	var sb strings.Builder
	sb.WriteString("hello")
	sb.WriteByte(' ')
	n, err := sb.WriteRune('é')
	sb.Write([]byte("!"))
	fmt.Println(sb.String(), sb.Len(), n, err)

	sb.Reset()
	fmt.Printf("%q %d\n", sb.String(), sb.Len())

	// A Builder is an io.Writer.
	b2 := &strings.Builder{}
	fmt.Fprintf(b2, "%s=%d;", "k", 3)
	io.WriteString(b2, "ws")
	fmt.Println(b2.String())

	// Passed as an io.Writer parameter.
	var w io.Writer = &sb
	fmt.Fprint(w, "through-iface")
	fmt.Println(sb.String())

	// Grow is a no-op that changes nothing observable.
	var g strings.Builder
	g.Grow(64)
	g.WriteString("x")
	fmt.Println(g.String(), g.Len())
}
