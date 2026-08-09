// Go's fixed-size array is a *value*, not a reference: `[N]T` is copied at
// every site a struct is, while `[]T` — the same heap object in go-rs — is
// shared at all of them. Each section below writes through what Go says is an
// independent copy and prints both sides, so a missed copy is a wrong number
// rather than an invisible aliasing. The last two sections are the other half
// of the rule: an array's *slice* elements stay shared, and an array is
// comparable (elementwise) and so usable as a map key, which a slice is not.
package main

import "fmt"

type pt struct{ x, y int }

type grid struct {
	a [2]int
	q [2]pt
	s []int
}

// A value parameter binds a copy: the caller's array is untouched, and the
// returned array carries the write.
func bump(a [3]int, d int) [3]int {
	a[0] += d
	return a
}

// A named result is a value too, and returning it copies.
func mk(v int) (r [2]int) {
	r[0] = v
	r[1] = v * 2
	return r
}

func main() {
	// assignment
	a := [3]int{1, 2, 3}
	b := a
	b[0] = 9
	fmt.Println("asg", a, b)

	// argument bind and return
	c := bump(a, 5)
	fmt.Println("call", a, c)
	d := mk(4)
	e := d
	e[1] = 7
	fmt.Println("ret", d, e)

	// nested arrays copy at every depth
	n := [2][2]int{{1, 2}, {3, 4}}
	m := n
	m[0][0] = 8
	m[1][1] = 8
	fmt.Println("nest", n, m)

	// an array of structs separates its structs
	as := [2]pt{{1, 2}, {3, 4}}
	bs := as
	bs[0].x = 9
	fmt.Println("astruct", as, bs)

	// an array-typed struct field is copied with the struct — and its own
	// elements with it — while a slice field keeps sharing
	g := grid{a: [2]int{1, 2}, q: [2]pt{{3, 4}, {5, 6}}, s: []int{7, 8}}
	h := g
	h.a[1] = 20
	h.q[1].y = 60
	h.s[0] = 70
	fmt.Println("field", g.a, h.a, g.q, h.q, g.s, h.s)

	// container read and store
	xs := [][2]int{{1, 2}, {3, 4}}
	r := xs[0]
	r[0] = 11
	fmt.Println("idx", xs[0], r)
	q := [2]int{5, 6}
	xs[1] = q
	q[0] = 12
	fmt.Println("store", xs[1], q)

	// append copies the appended element; append(dst, src...) copies each
	ys := append(xs, q)
	q[1] = 13
	zs := append([][2]int{}, xs...)
	zs[0][0] = 14
	fmt.Println("append", ys[2], xs[0], zs[0])

	// a map value read is a copy
	mv := map[string][2]int{"k": {1, 2}}
	v := mv["k"]
	v[0] = 15
	fmt.Println("map", mv["k"], v)

	// a channel send copies
	ch := make(chan [3]int, 1)
	ch <- a
	rv := <-ch
	rv[1] = 16
	fmt.Println("chan", a, rv)

	// `range` walks a copy of the array, so a write inside the loop is not
	// seen by the remaining iterations; the value binding is a copy too
	sum := 0
	for i, ev := range a {
		if i == 0 {
			a[1] = 100
		}
		sum += ev
	}
	fmt.Println("rng", sum, a)
	for _, ev := range xs {
		ev[0] = -1
	}
	fmt.Println("rngelem", xs)

	// zero values: N element zeros, at every depth and in a struct field
	var z [3]int
	var zz [2][2]int
	var zg grid
	fmt.Println("zero", z, zz, zg)

	// an array of slices copies the array and shares the slices
	sh := [2][]int{{1, 2}, {3}}
	sk := sh
	sk[0][0] = 40
	sk[1] = []int{8}
	fmt.Println("share", sh, sk)

	// slicing an array yields a slice over that array's storage
	sa := a[:]
	sa[2] = 30
	fmt.Println("slice", a, sa, len(a), cap(a))

	// comparison is elementwise, so an array is a usable map key
	fmt.Println("eq", [2]int{1, 2} == [2]int{1, 2}, [2]int{1, 2} == [2]int{1, 3}, n == m)
	km := map[[2]int]string{{1, 2}: "a"}
	km[[2]int{3, 4}] = "b"
	fmt.Println("key", km[[2]int{1, 2}], km[[2]int{3, 4}], len(km))
}
