package main

// Writer-directed output. `fmt.Fprint*(w, …)` is `w.Write([]byte(fmt.Sprint*(…)))`
// and nothing else, so it needs no runtime support — only the rewrite, and a
// writer whose `Write` actually reaches the caller's buffer.
//
// That second half is the part that used to be missing: a `*T` passed as an
// argument was copied, so every writer accumulated into a copy and silently
// discarded. The cases below all pass the writer somewhere before writing
// through it.

import (
	"fmt"
	"io"
)

type buf struct{ b []byte }

func (w *buf) Write(p []byte) (int, error) {
	w.b = append(w.b, p...)
	return len(p), nil
}

func (w *buf) String() string { return string(w.b) }

// A writer that also implements StringWriter, which io.WriteString prefers.
type strbuf struct{ s string }

func (w *strbuf) Write(p []byte) (int, error) {
	w.s += string(p)
	return len(p), nil
}

func (w *strbuf) WriteString(s string) (int, error) {
	w.s += "<" + s + ">"
	return len(s), nil
}

// A counting writer: proves the return values are the writer's own.
type counter struct{ n int }

func (c *counter) Write(p []byte) (int, error) {
	c.n += len(p)
	return len(p), nil
}

func emit(w io.Writer, s string) (int, error) { return fmt.Fprint(w, s) }

func banner(w io.Writer) { fmt.Fprintf(w, "[%s:%d]\n", "hdr", 1) }

func main() {
	b := &buf{}

	// The three forms, and their (n, err) results.
	n1, e1 := fmt.Fprintf(b, "%s=%d\n", "x", 7)
	n2, e2 := fmt.Fprintln(b, "line", 2)
	n3, e3 := fmt.Fprint(b, "raw", 3, "\n")
	fmt.Print(b.String())
	fmt.Println("counts", n1, n2, n3, e1, e2, e3)

	// Through an interface parameter, and from a function that forwards the
	// writer's own results.
	n4, e4 := emit(b, "via-iface\n")
	banner(b)
	fmt.Print(b.String()[len("x=7\nline 2\nraw3\n"):])
	fmt.Println("emit", n4, e4)

	// io.WriteString prefers a StringWriter and falls back to Write.
	sb := &strbuf{}
	m1, _ := io.WriteString(sb, "pref")
	pb := &buf{}
	m2, _ := io.WriteString(pb, "fall")
	fmt.Println("writestring", sb.s, string(pb.b), m1, m2)

	// A writer held in an interface variable, and one in a slice.
	var w io.Writer = &counter{}
	fmt.Fprintf(w, "%05d", 42)
	ws := []io.Writer{&counter{}, &counter{}}
	for _, each := range ws {
		fmt.Fprint(each, "abc")
	}
	fmt.Println("counter", w.(*counter).n, ws[0].(*counter).n, ws[1].(*counter).n)

	// The same writer passed down two levels still accumulates in one place.
	deep := &buf{}
	banner(deep)
	emit(deep, "second\n")
	fmt.Print(deep.String())

	// Fprintln's spacing and newline match Sprintln's, and Fprint's match
	// Sprint's — including the rule that Fprint only spaces between operands
	// when neither is a string.
	c := &buf{}
	fmt.Fprintln(c, 1, "a", true)
	fmt.Fprint(c, 1, 2, "a", "b", 3, "\n")
	fmt.Print(c.String())

	// A zero-length write and a write of a formatted empty string.
	z := &counter{}
	nz, _ := fmt.Fprint(z, "")
	fmt.Println("empty", z.n, nz)
}
