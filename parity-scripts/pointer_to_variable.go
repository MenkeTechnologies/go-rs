package main

// `&x` on an existing *variable*. `&T{…}` can mark its own handle a pointer,
// because that handle is born one and nothing else refers to it; `&x` cannot —
// the handle is `x`'s, and marking it would make `x` itself compare by identity
// where Go compares field by field. So `&x` allocates a pointer *object* whose
// target is the variable's handle, which leaves `x` exactly as it was and gives
// the pointer an address of its own to bind, store and compare by.
//
// What that has to get right at once: a pointer survives a rebind, an argument
// bind and a container store without copying; `*p = v` overwrites the pointee in
// place so every other pointer to it sees the write; two pointers to one
// variable are equal and to different variables are not; and the pointee itself
// still copies on assignment and still compares field by field.

import "fmt"

type pt struct{ X, Y int }

func bump(p *pt) { p.X = 7 }

func esc() *pt { v := pt{1, 2}; return &v }

func main() {
	// &x then rebind
	x := pt{1, 2}
	p := &x
	q := p
	q.X = 7
	fmt.Println("rebind", x.X, p.X, q.X)

	// &x passed as an argument through a variable
	z := pt{1, 2}
	pz := &z
	bump(pz)
	fmt.Println("var-addr-arg", z.X)

	// *p = v writes through
	w := pt{1, 2}
	pw := &w
	*pw = pt{9, 9}
	fmt.Println("deref-assign", w.X, w.Y)

	// p == q: same variable vs different
	a := pt{1, 2}
	b := pt{1, 2}
	p1, p2, p3 := &a, &a, &b
	fmt.Println("ptr-eq", p1 == p2, p1 == p3)

	// field-wise == on the pointee stays correct
	fmt.Println("value-eq", a == b, a == pt{1, 2}, a == pt{9, 9})

	// &x into a slice, a map, a struct field
	s := []*pt{&a}
	s[0].Y = 30
	m := map[string]*pt{"k": &a}
	m["k"].Y = 31
	type holder struct{ P *pt }
	h := holder{P: &a}
	h.P.Y = 32
	fmt.Println("containers", a.Y)

	// &x escaping a function
	e := esc()
	e.X = 42
	fmt.Println("escape", e.X, e.Y)

	// value semantics still copy
	c := a
	c.X = 99
	fmt.Println("value-copy", a.X, c.X)

	// *p read is a copy
	d := *p1
	d.X = 77
	fmt.Println("deref-read", a.X, d.X)
}
