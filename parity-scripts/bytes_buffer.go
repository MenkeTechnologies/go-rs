package main

// `bytes.Buffer` — somewhere to accumulate output, and an `io.Writer`.
//
// Unlike `strings.Builder`, `bytes` is not a native host package, so this comes
// in through the ordinary vendored-source path that `errors` and `io` use:
// real Go, parsed and linked. `Cap` and `Grow` are deliberately absent for the
// same reason `Builder.Cap` is — go-rs cannot reserve capacity, so a program
// calling them gets a compile error rather than a confident wrong number.

import (
	"bytes"
	"fmt"
	"io"
)

func main() {
	var b bytes.Buffer
	b.WriteString("hello")
	b.WriteByte(' ')
	n, err := b.WriteRune('é')
	b.Write([]byte("!"))
	fmt.Println(b.String(), b.Len(), n, err, len(b.Bytes()))

	fmt.Fprintf(&b, " %s=%d", "k", 3)
	io.WriteString(&b, " ws")
	fmt.Println(b.String())

	b.Reset()
	fmt.Printf("%q %d\n", b.String(), b.Len())

	p := bytes.NewBufferString("seed")
	p.WriteString("-more")
	fmt.Println(p.String(), p.Len())

	q := bytes.NewBuffer([]byte("ab"))
	var w io.Writer = q
	fmt.Fprint(w, "cd")
	fmt.Println(q.String())
}
