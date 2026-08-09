package main

import (
	"fmt"
	"sort"
)

// Go struct value semantics: a struct is copied at every assignment, argument
// bind, return, container store, container read, range binding, channel send
// and value-receiver call — transitively, through nested struct fields — while
// its pointer, slice and map fields stay shared. Checked byte-for-byte against
// the real `go`.

type leaf struct{ N int }

type mid struct {
	L leaf
	N int
}

type rich struct {
	M mid
	P *leaf
	S []int
	H map[string]int
	N int
}

func (r rich) byValue() { r.N = 900; r.M.N = 901; r.M.L.N = 902 }

func (r *rich) byPointer() { r.N = 700; r.M.L.N = 701 }

func takes(r rich) rich {
	r.N = 100
	r.M.N = 101
	r.M.L.N = 102
	return r
}

func main() {
	base := rich{
		M: mid{L: leaf{1}, N: 2},
		P: &leaf{3},
		S: []int{4},
		H: map[string]int{"k": 5},
		N: 6,
	}

	// Assignment copies every depth of the value part.
	cp := base
	cp.N = 60
	cp.M.N = 20
	cp.M.L.N = 10
	fmt.Println("assign:", base.N, base.M.N, base.M.L.N, "|", cp.N, cp.M.N, cp.M.L.N)

	// The reference-typed fields are shared by that same copy.
	cp.P.N = 30
	cp.S[0] = 40
	cp.H["k"] = 50
	fmt.Println("shared:", base.P.N, base.S[0], base.H["k"])

	// Argument bind and return are copies.
	got := takes(base)
	fmt.Println("call:", base.N, base.M.N, base.M.L.N, "|", got.N, got.M.N, got.M.L.N)

	// A value receiver cannot write through to the caller; a pointer one must.
	base.byValue()
	fmt.Println("value-recv:", base.N, base.M.N, base.M.L.N)
	base.byPointer()
	fmt.Println("ptr-recv:", base.N, base.M.L.N)

	// Storing into and reading out of a slice both copy.
	xs := []rich{base}
	xs = append(xs, base)
	xs[0].N = 11
	xs[0].M.L.N = 12
	out := xs[1]
	out.N = 13
	out.M.L.N = 14
	fmt.Println("slice:", base.N, base.M.L.N, xs[1].N, xs[1].M.L.N, out.N, out.M.L.N)

	// Same for a map.
	hm := map[string]rich{"a": base}
	hm["b"] = base
	got2 := hm["b"]
	got2.N = 15
	got2.M.L.N = 16
	fmt.Println("map:", base.N, base.M.L.N, hm["b"].N, hm["b"].M.L.N, got2.N, got2.M.L.N)

	// A range variable is a copy, so a read-only walk stays read-only.
	for _, v := range xs {
		v.N = 17
		v.M.L.N = 18
	}
	for _, v := range hm {
		v.N = 19
	}
	fmt.Println("range:", xs[0].N, xs[0].M.L.N, xs[1].N, hm["a"].N, hm["b"].N)

	// Spreading into append copies each element of the source.
	ys := append([]rich{}, xs...)
	ys[0].N = 21
	ys[0].M.L.N = 22
	fmt.Println("spread:", xs[0].N, xs[0].M.L.N, ys[0].N, ys[0].M.L.N)

	// A channel transfers a copy.
	ch := make(chan rich, 1)
	ch <- base
	recv := <-ch
	recv.N = 23
	recv.M.L.N = 24
	fmt.Println("chan:", base.N, base.M.L.N, recv.N, recv.M.L.N)

	// Through an interface, and back out of a type assertion.
	var any1 any = base
	asserted := any1.(rich)
	asserted.N = 25
	asserted.M.L.N = 26
	fmt.Println("iface:", base.N, base.M.L.N, asserted.N, asserted.M.L.N)

	// Equality is field-wise at every depth.
	e1 := mid{L: leaf{1}, N: 2}
	e2 := mid{L: leaf{1}, N: 2}
	e3 := e1
	e3.L.N = 99
	fmt.Println("eq:", e1 == e2, e1 == e3, e2 == e3)

	// A closure argument is bound the same way as a function's.
	mutate := func(r rich) { r.N = 27; r.M.L.N = 28 }
	mutate(base)
	fmt.Println("closure:", base.N, base.M.L.N)

	// Nested containers: the element type has to be recovered through both
	// index steps, including a map whose value type carries its own brackets.
	grid := [][]mid{{{L: leaf{41}, N: 42}}}
	cell := grid[0][0]
	cell.L.N = 43
	byName := map[string][]mid{"g": {{L: leaf{44}, N: 45}}}
	item := byName["g"][0]
	item.L.N = 46
	fmt.Println("nested:", grid, cell.L.N, byName, item.L.N)

	// `sort.Slice` reorders in place through the comparator's index reads.
	unsorted := []mid{{L: leaf{3}, N: 3}, {L: leaf{1}, N: 1}, {L: leaf{2}, N: 2}}
	sort.Slice(unsorted, func(i, j int) bool { return unsorted[i].N < unsorted[j].N })
	fmt.Println("sort:", unsorted)

	// Every element of a zero-filled container is its own struct, so writing
	// through one slot leaves the others zero.
	var arr [3]mid
	arr[0].N = 31
	arr[1].L.N = 32
	fmt.Println("zero-array:", arr)
	made := make([]mid, 3)
	made[0].N = 33
	made[1].L.N = 34
	fmt.Println("zero-make:", made)
	capped := make([]mid, 2, 4)
	capped[0].L.N = 35
	fmt.Println("zero-make-cap:", capped, len(capped), cap(capped))

	// `base` itself is not printed whole: its `*leaf` field renders as a hex
	// address, which is not reproducible between two runs of `go` either.
	fmt.Println("final:", base.M, base.S, base.H, base.N, cp.M, got.M.L, *base.P)
}
