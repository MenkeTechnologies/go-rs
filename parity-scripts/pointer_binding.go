package main

// Go copies a struct *value* at every bind site — an assignment, an argument, a
// return, a container store, a `range` binding — and copies nothing at all
// through a pointer. go-rs models both as the same heap handle, so the two
// cases reach the run time indistinguishable except for the `by_ref` mark that
// `&T{…}` and `new(T)` leave on the handle. A bind honours that mark; `*p` and
// a value-receiver method deliberately ignore it, because both are asking for
// the pointed-to value.
//
// Every line below is a place where copying a pointer, or failing to copy a
// value, is observable.

import "fmt"

type pt struct{ X, Y int }

type node struct {
	V    int
	Next *node
}

type holder struct {
	P *pt
	V pt
}

func bump(q *pt) { q.X = 7 }

func bumpValue(q pt) { q.X = 99 }

func (p *pt) SetX(n int) { p.X = n }

func (p pt) WithX(n int) pt { p.X = n; return p }

func give() *pt { return &pt{1, 2} }

func main() {
	// Assignment of a pointer shares; assignment of a value copies.
	p := &pt{1, 2}
	q := p
	q.X = 7
	a := pt{1, 2}
	b := a
	b.X = 9
	fmt.Println("assign", p.X, q.X, a.X, b.X)

	// Argument bind: a pointer parameter writes through, a value one does not.
	r := &pt{1, 2}
	bump(r)
	v := pt{1, 2}
	bumpValue(v)
	fmt.Println("args", r.X, v.X)

	// A pointer stored in a slice, an array and a map stays the same pointer.
	s := &pt{1, 2}
	sl := []*pt{s}
	sl[0].X = 5
	arr := [1]*pt{s}
	arr[0].Y = 6
	m := map[string]*pt{"k": s}
	m["k"].X = 8
	fmt.Println("containers", s.X, s.Y)

	// The value forms of the same three still copy.
	vs := []pt{{1, 2}}
	got := vs[0]
	got.X = 50
	vm := map[string]pt{"k": {1, 2}}
	gotm := vm["k"]
	gotm.X = 60
	fmt.Println("value-containers", vs[0].X, got.X, vm["k"].X, gotm.X)

	// A returned pointer is the one that was allocated.
	g := give()
	h := g
	h.Y = 42
	fmt.Println("return", g.Y)

	// `*p` asks for the value, so it copies — the inverse of the rule above.
	d := &pt{1, 2}
	e := *d
	e.X = 77
	fmt.Println("deref", d.X, e.X)

	// A value receiver gets a copy even when called through a pointer; a
	// pointer receiver writes through even when called on an addressable value.
	pr := &pt{1, 2}
	_ = pr.WithX(31)
	fmt.Println("value-recv", pr.X)
	pr.SetX(12)
	fmt.Println("ptr-recv", pr.X)

	// A pointer *field* of a copied struct is shared; a value field is not.
	hold := holder{P: &pt{1, 2}, V: pt{3, 4}}
	cp := hold
	cp.P.X = 21
	cp.V.X = 22
	fmt.Println("fields", hold.P.X, hold.V.X, cp.V.X)

	// A self-referential pointer chain survives being bound around.
	n1 := &node{V: 1}
	n2 := &node{V: 2}
	n1.Next = n2
	alias := n1
	alias.Next.V = 20
	fmt.Println("chain", n1.Next.V, n2.V)

	// `range` over a slice of pointers binds the pointer, not a copy of what it
	// points at; over a slice of values it binds a copy.
	ps := []*pt{&pt{1, 2}, &pt{3, 4}}
	for _, e := range ps {
		e.X += 10
	}
	vsl := []pt{{1, 2}, {3, 4}}
	for _, e := range vsl {
		e.X += 10
	}
	fmt.Println("range", ps[0].X, ps[1].X, vsl[0].X, vsl[1].X)

	// A pointer through an interface keeps its identity.
	var any1 interface{} = p
	if pp, ok := any1.(*pt); ok {
		pp.Y = 64
	}
	fmt.Println("iface", p.Y)

	// Two pointers to separately allocated equal values stay distinct keys, and
	// the same pointer bound twice stays one.
	k1 := &pt{1, 2}
	k2 := k1
	km := map[*pt]string{}
	km[k1] = "first"
	km[k2] = "second"
	fmt.Println("ptr-key", len(km), km[k1])

	// new(T) is a pointer just like &T{}.
	np := new(pt)
	np2 := np
	np2.X = 13
	fmt.Println("new", np.X)

	// A pointer passed through a closure parameter still writes through.
	cl := func(z *pt) { z.Y = 88 }
	cl(np)
	fmt.Println("closure", np.Y)

	// A pointer sent over a channel is the same pointer.
	ch := make(chan *pt, 1)
	ch <- np
	rp := <-ch
	rp.X = 90
	fmt.Println("chan", np.X)

	// A pointer appended to a slice is the same pointer.
	ap := []*pt{}
	ap = append(ap, np)
	ap[0].Y = 91
	fmt.Println("append", np.Y)
}
