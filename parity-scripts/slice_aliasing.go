// Which slice operations share a backing array and which reallocate — the part of
// `append` and `copy` that is observable through a second slice, including the
// two overlapping-`copy` directions and appending a slice to itself.
package main

import "fmt"

func main() {
	// append into spare capacity aliases the backing array
	a := make([]int, 3, 8)
	a[0], a[1], a[2] = 1, 2, 3
	b := a[:2]
	b = append(b, 99)
	fmt.Println(a, b, len(a), len(b))

	// append past capacity reallocates and stops aliasing
	c := make([]int, 2, 2)
	c[0], c[1] = 7, 8
	d := append(c, 9)
	d[0] = 100
	fmt.Println(c, d)

	// three-index slice caps the view
	e := []int{1, 2, 3, 4}
	f := e[0:2:2]
	f = append(f, 55)
	fmt.Println(e, f)

	// a sub-slice write is visible through the parent
	g := []int{1, 2, 3}
	h := g[1:]
	h[0] = 20
	fmt.Println(g, h)

	// append(dst, src...) copies
	i := []int{1, 2}
	j := []int{3, 4}
	k := append(i[:1:1], j...)
	j[0] = 30
	fmt.Println(i, j, k)

	// copy overlapping
	l := []int{1, 2, 3, 4, 5}
	copy(l[1:], l[:4])
	fmt.Println(l)
	m := []int{1, 2, 3, 4, 5}
	copy(m[:4], m[1:])
	fmt.Println(m)

	// append a slice to itself
	n := []int{1, 2, 3}
	n = append(n, n...)
	fmt.Println(n, len(n))
}
