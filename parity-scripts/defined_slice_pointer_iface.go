// A pointer to a defined slice or map type in an interface: `*T` and `T` are
// distinct dynamic types, a value method called through the `*T` reads the
// pointee, a pointer method retargets the pointer the program holds, and an
// assertion back to `*T` yields that same pointer. This is the shape every
// `container/heap` user writes; the heap functions below are Go's own.
package main

import (
	"fmt"
	"sort"
	"strings"
)

type Interface interface {
	sort.Interface
	Push(x any)
	Pop() any
}

func Init(h Interface) {
	n := h.Len()
	for i := n/2 - 1; i >= 0; i-- {
		down(h, i, n)
	}
}

func Push(h Interface, x any) {
	h.Push(x)
	up(h, h.Len()-1)
}

func Pop(h Interface) any {
	n := h.Len() - 1
	h.Swap(0, n)
	down(h, 0, n)
	return h.Pop()
}

func up(h Interface, j int) {
	for {
		i := (j - 1) / 2 // parent
		if i == j || !h.Less(j, i) {
			break
		}
		h.Swap(i, j)
		j = i
	}
}

func down(h Interface, i0, n int) bool {
	i := i0
	for {
		j1 := 2*i + 1
		if j1 >= n || j1 < 0 { // j1 < 0 after int overflow
			break
		}
		j := j1 // left child
		if j2 := j1 + 1; j2 < n && h.Less(j2, j1) {
			j = j2 // = 2*i + 2  // right child
		}
		if !h.Less(j, i) {
			break
		}
		h.Swap(i, j)
		i = j
	}
	return i > i0
}

type IntHeap []int

func (h IntHeap) Len() int           { return len(h) }
func (h IntHeap) Less(i, j int) bool { return h[i] < h[j] }
func (h IntHeap) Swap(i, j int)      { h[i], h[j] = h[j], h[i] }
func (h *IntHeap) Push(x any)        { *h = append(*h, x.(int)) }
func (h *IntHeap) Pop() any {
	old := *h
	n := len(old)
	x := old[n-1]
	*h = old[0 : n-1]
	return x
}

type Names []string

func (n Names) String() string { return strings.Join(n, "+") }

type Errs []string

func (e *Errs) Error() string { return fmt.Sprint(len(*e), " errors") }

type Set map[string]bool

func (s Set) Has(k string) bool { return s[k] }
func (s *Set) Reset()            { *s = Set{} }

type Haser interface{ Has(string) bool }

type Lener interface{ Len() int }

func main() {
	h := &IntHeap{5, 2, 8}
	Init(h)
	fmt.Println(*h)
	Push(h, 3)
	fmt.Println(*h, h.Len())
	for h.Len() > 0 {
		fmt.Print(Pop(h), " ")
	}
	fmt.Println()

	var l Lener = &IntHeap{1, 2, 3}
	fmt.Println(l.Len())
	var a any = h
	fmt.Printf("%v %T\n", a, a)
	p := a.(*IntHeap)
	p.Push(7)
	fmt.Println(*h, p == h, a == any(h))
	_, isVal := a.(IntHeap)
	fmt.Println(isVal)

	n := &Names{"a", "b"}
	var s fmt.Stringer = n
	fmt.Println(s, s.String())
	fmt.Printf("%v %s %T\n", s, s, s)
	e := &Errs{"x", "y"}
	var err error = e
	fmt.Println(err)
	if pe, ok := err.(*Errs); ok {
		*pe = append(*pe, "z")
	}
	fmt.Println(err, len(*e))

	set := &Set{"k": true}
	var hs Haser = set
	fmt.Println(hs.Has("k"), hs.Has("q"))
	switch v := hs.(type) {
	case Set:
		fmt.Println("value")
	case *Set:
		v.Reset()
		fmt.Println("pointer", len(*set), v.Has("k"))
	}
	var hv Haser = Set{"z": true}
	_, isPtr := hv.(*Set)
	_, isVal = hv.(Set)
	fmt.Println(isPtr, isVal)

	var np *Names
	var a2 any = np
	fmt.Println(a2 == nil, a2)
	fmt.Printf("%T\n", a2)
	_, ok := any(Errs{}).(error)
	fmt.Println(ok)
	_, ok = any(&Errs{}).(error)
	fmt.Println(ok)
}
