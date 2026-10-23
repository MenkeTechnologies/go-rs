// The language support Go's fmt/scan.go leans on, each shape on its own:
// an interface embedding another (and its methods' result counts), a type
// switch over pointers to basic types of every width, `&err` of an interface
// variable rebound through the pointer, an array sized by another package's
// constant, a pointer method called through a local `p := &v` of a defined
// non-struct type, and a field read through a pointer bound by a tuple
// destructure.
package main

import (
	"errors"
	"fmt"
	"io"
	"unicode/utf8"
)

type RuneSource interface {
	ReadRune() (r rune, size int, err error)
}

type RuneBuffer interface {
	RuneSource
	UnreadRune() error
}

type letters struct{ s string }

func (l *letters) ReadRune() (rune, int, error) {
	if l.s == "" {
		return 0, 0, io.EOF
	}
	r, n := utf8.DecodeRuneInString(l.s)
	l.s = l.s[n:]
	return r, n, nil
}

func (l *letters) UnreadRune() error { return errors.New("no") }

type onlyRead struct{}

func (onlyRead) ReadRune() (rune, int, error) { return 'x', 1, nil }

type holder struct{ src RuneBuffer }

func newHolder(s string) (*holder, int) { return &holder{&letters{s}}, len(s) }

func kind(arg any) string {
	switch v := arg.(type) {
	case *int:
		*v += 1
		return "*int"
	case *int8:
		*v = -8
		return "*int8"
	case *int64:
		*v = 1 << 40
		return "*int64"
	case *uint16:
		*v = 65535
		return "*uint16"
	case *float32:
		*v = 0.5
		return "*float32"
	case *float64:
		*v = 2.25
		return "*float64"
	case *string:
		*v += "!"
		return "*string"
	case *bool:
		*v = !*v
		return "*bool"
	case *[]byte:
		*v = append(*v, 'z')
		return "*[]byte"
	}
	return fmt.Sprintf("%T", arg)
}

func recoverInto(errp *error) {
	if e := recover(); e != nil {
		if err, ok := e.(error); ok {
			*errp = err
		}
	}
}

func fails() (n int, err error) {
	defer recoverInto(&err)
	n = 3
	panic(io.ErrUnexpectedEOF)
}

type Stack []int

func (s *Stack) Push(v int) { *s = append(*s, v) }

type Count int

func (c *Count) Inc() { *c++ }

type Name string

func (n *Name) Upper2() string { return string((*n)[:2]) }

func main() {
	var b RuneBuffer = &letters{"héllo"}
	r, size, err := b.ReadRune()
	fmt.Println(r, size, err)
	r, size, err = b.ReadRune()
	fmt.Println(string(r), size, err)
	var src any = onlyRead{}
	_, isBuffer := src.(RuneBuffer)
	_, isSource := src.(RuneSource)
	fmt.Println(isBuffer, isSource)

	var (
		i   int = 41
		i8  int8
		i64 int64
		u16 uint16
		f32 float32
		f64 float64
		s   = "hey"
		t   bool
		bs  []byte
	)
	p := new(int)
	fmt.Println(kind(&i), kind(&i8), kind(&i64), kind(&u16), kind(&f32), kind(&f64), kind(&s), kind(&t), kind(&bs), kind(p), kind(i))
	fmt.Println(i, i8, i64, u16, f32, f64, s, t, string(bs), *p)

	fmt.Println(fails())
	var e error
	func(ep *error) { *ep = errors.New("set") }(&e)
	fmt.Println(e)

	var buf [utf8.UTFMax]byte
	var tbl [utf8.RuneSelf / 64]int
	fmt.Println(len(buf), len(tbl), buf)

	var st Stack
	ps := &st
	ps.Push(1)
	ps.Push(2)
	var c Count
	pc := &c
	pc.Inc()
	pc.Inc()
	nm := Name("gopher")
	pn := &nm
	fmt.Println(st, c, pn.Upper2())

	h, n := newHolder("ab")
	r, size, err = h.src.ReadRune()
	fmt.Println(n, string(r), size, err)
}
