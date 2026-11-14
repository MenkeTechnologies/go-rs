// `io.Copy` and its family, and the adapters over `Reader` / `Writer`: Go's own
// `io.go` / `multi.go` bodies, including `Copy`'s `WriterTo` / `ReaderFrom`
// short-cuts, `ErrShortWrite` and a reader's own error.
package main

import (
	"bytes"
	"errors"
	"fmt"
	"io"
	"os"
	"strings"
)

type upper struct{ w io.Writer }

func (u upper) Write(p []byte) (int, error) {
	return u.w.Write(bytes.ToUpper(p))
}

type short struct{}

func (short) Write(p []byte) (int, error) { return len(p) - 1, nil }

type failing struct{}

func (failing) Read(p []byte) (int, error) { return 0, errors.New("boom") }

func main() {
	var buf bytes.Buffer
	n, err := io.Copy(&buf, strings.NewReader("copied text"))
	fmt.Println(n, err, buf.String())
	n, err = io.CopyN(os.Stdout, strings.NewReader("0123456789"), 4)
	fmt.Println()
	fmt.Println(n, err)
	n, err = io.CopyN(io.Discard, strings.NewReader("ab"), 5)
	fmt.Println(n, err)
	var log strings.Builder
	tee := io.TeeReader(strings.NewReader("tee data"), &log)
	all, _ := io.ReadAll(tee)
	fmt.Println(string(all), "|", log.String())
	mr := io.MultiReader(strings.NewReader("one "), strings.NewReader("two "), strings.NewReader("three"))
	io.Copy(upper{os.Stdout}, mr)
	fmt.Println()
	n, err = io.Copy(short{}, strings.NewReader("xyz"))
	fmt.Println(n, err)
	n, err = io.Copy(io.Discard, failing{})
	fmt.Println(n, err)
	rc := io.NopCloser(strings.NewReader("nop"))
	fmt.Println(rc.Close())
	b, _ := io.ReadAll(rc)
	fmt.Println(string(b))
	lr := &io.LimitedReader{R: strings.NewReader("limited reader"), N: 7}
	b, _ = io.ReadAll(lr)
	fmt.Println(string(b), lr.N)
	p := make([]byte, 4)
	k, err := io.ReadFull(strings.NewReader("ab"), p)
	fmt.Println(k, err)
	var w io.Writer = io.MultiWriter(&buf, io.Discard)
	io.WriteString(w, "!")
	fmt.Println(buf.String())
	n, err = io.CopyBuffer(os.Stdout, io.LimitReader(strings.NewReader("buffered copy\n"), 100), make([]byte, 3))
	fmt.Println(n, err)
	var rw io.ReadWriter = &buf
	fmt.Printf("%T\n", rw)
}
