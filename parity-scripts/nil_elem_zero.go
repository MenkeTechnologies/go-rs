// The zero value of a pointer, interface, function or channel element is nil:
// `make([]*T, n)`, `var a [N]error`, `make([]Shape, n)` and friends are filled
// with nils — which compare equal to nil, print as <nil>, and are what a
// "first use allocates" pattern tests for.
package main

import "fmt"

type node struct {
	val  int
	next *node
}

type Shape interface{ Area() float64 }

type sq struct{ s float64 }

func (q sq) Area() float64 { return q.s * q.s }

func main() {
	ptrs := make([]*node, 3)
	fmt.Println(ptrs[0] == nil, ptrs)
	for i := range ptrs {
		if ptrs[i] == nil {
			ptrs[i] = &node{val: i}
		}
	}
	fmt.Println(ptrs[2].val, ptrs[1].next == nil)

	var errs [2]error
	fmt.Println(errs[0] == nil, errs)

	shapes := make([]Shape, 2)
	fmt.Println(shapes[1] == nil)
	shapes[0] = sq{3}
	for _, s := range shapes {
		if s != nil {
			fmt.Println(s.Area())
		} else {
			fmt.Println("empty")
		}
	}

	fns := make([]func() int, 2)
	fmt.Println(fns[0] == nil)

	chans := make([]chan int, 1)
	fmt.Println(chans[0] == nil)

	anys := make([]any, 2)
	fmt.Println(anys[0] == nil, anys)

	buckets := make([][]*node, 2)
	buckets[0] = make([]*node, 1)
	fmt.Println(buckets[0][0] == nil, buckets[1] == nil)

	// A slice or map element zeroes to its typed nil, which is what the
	// conversion `[]T(nil)` spells.
	var grid [2][]int
	maps := make([]map[string]int, 1)
	fmt.Println(grid[0] == nil, grid, maps[0] == nil, maps)
	grid[1] = append(grid[1], 4)
	fmt.Println(grid, len(grid[0]))
	none := []int(nil)
	fmt.Printf("%v %d %#v %#v\n", none == nil, len(none), none, map[string]bool(nil))
}
