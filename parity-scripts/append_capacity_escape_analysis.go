// An append to a slice that never leaves its frame grows from a 32-byte stack
// buffer, so the first capacities are 4, 8, … where a heap slice's are 1, 2, 4 …;
// whether a slice leaves its frame is Go's escape analysis: a global store, an
// `fmt` operand, a goroutine capture, or a return that is kept does it, and a
// call into a function that only reads the slice does not.
package main

import (
	"fmt"
	"slices"
	"sort"
	"strings"
)

func total(xs []int) int {
	t := 0
	for _, x := range xs {
		t += x
	}
	return t
}

func keep(xs []int)         { global = xs }
func ret(xs []int) []int    { return xs }
func first(xs []int) int    { return xs[0] }
func sub(xs []int) int      { return len(xs[1:]) }
func app(xs []int) []int    { return append(xs, 1) }
func pr(xs []int)           { fmt.Println(len(xs)) }
func rec(xs []int, n int) int {
	if n == 0 {
		return len(xs)
	}
	return rec(xs, n-1)
}
func viaClosure(xs []int) int {
	f := func() int { return len(xs) }
	return f()
}

var global []int

type H struct{ s []int }

func (h *H) set(xs []int) { h.s = xs }
func (h H) get(xs []int) int { return len(xs) }

func main() {
	var a []int
	a = append(a, 1)
	fmt.Println("plain", cap(a))

	var b []int
	b = append(b, 1)
	_ = total(b)
	fmt.Println("total", cap(b))

	var c []int
	c = append(c, 1)
	keep(c)
	fmt.Println("keep", cap(c))

	var d []int
	d = append(d, 1)
	_ = ret(d)
	fmt.Println("ret", cap(d))

	var e []int
	e = append(e, 1)
	_ = first(e)
	fmt.Println("first", cap(e))

	var f []int
	f = append(f, 1)
	sort.Ints(f)
	fmt.Println("sort.Ints", cap(f))

	var g []string
	g = append(g, "x")
	_ = strings.Join(g, ",")
	fmt.Println("join", cap(g))

	var h []int
	h = append(h, 1)
	hh := H{h}
	fmt.Println("structlit", cap(h), len(hh.s))

	var k []int
	k = append(k, 1)
	fn := func() int { return len(k) }
	fmt.Println("closure", cap(k), fn())

	var l []int
	l = append(l, 1)
	m := l[0:1]
	fmt.Println("subslice", cap(l), len(m))

	var n []int
	n = append(n, 1)
	fmt.Println("println", cap(n), n)

	var o []int
	o = append(o, 1)
	_ = sub(o)
	fmt.Println("sub", cap(o))

	var p []int
	p = append(p, 1)
	_ = app(p)
	fmt.Println("app", cap(p))

	var q []int
	q = append(q, 1)
	pr(q)
	fmt.Println("pr", cap(q))

	var r []int
	r = append(r, 1)
	_ = rec(r, 3)
	fmt.Println("rec", cap(r))

	var s []int
	s = append(s, 1)
	_ = viaClosure(s)
	fmt.Println("viaClosure", cap(s))

	var t []int
	t = append(t, 1)
	var hp H
	hp.set(t)
	fmt.Println("method-leak", cap(t))

	var u []int
	u = append(u, 1)
	fmt.Println("method-noleak", cap(u), hp.get(u))

	var v []int
	v = append(v, 1)
	slices.Sort(v)
	fmt.Println("slices.Sort", cap(v))

	var w []int
	w = append(w, 1)
	fmt.Println("slices.Contains", cap(w), slices.Contains(w, 1))

	var x []int
	x = append(x, 1)
	y := x
	fmt.Println("copyvar", cap(x), len(y))

	var z []int
	z = append(z, 1)
	copy(z, []int{2})
	fmt.Println("copy", cap(z), z == nil)

	var aa []int
	aa = append(aa, 1)
	for range aa {
	}
	aa2 := append(aa, 2)
	fmt.Println("append-other", cap(aa), cap(aa2))

	var bb [][]int
	bb = append(bb, nil)
	fmt.Println("2d", cap(bb))
	var cc []int
	for i := 0; i < 3; i++ {
		cc = append(cc, i)
		fmt.Print(cap(cc), ",")
	}
	cc = cc[:0]
	cc = append(cc, 9)
	fmt.Println(cap(cc), cc[0])
}
