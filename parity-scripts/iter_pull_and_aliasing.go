package main

import (
	"fmt"
	"iter"
	"maps"
	"slices"
)

func count(n int) iter.Seq[int] {
	return func(yield func(int) bool) {
		for i := 0; i < n; i++ {
			if !yield(i) {
				return
			}
		}
	}
}

func pairs() iter.Seq2[string, int] {
	return func(yield func(string, int) bool) {
		_ = yield("a", 1) && yield("b", 2)
	}
}

func main() {
	// Insert / Replace of a slice into itself take Go's aliasing path
	// (`overlaps`), which must see the shared backing array.
	a := []int{1, 2, 3, 4, 5}
	a = slices.Insert(a, 2, a[1:3]...)
	fmt.Println(a)
	b := make([]int, 3, 10)
	copy(b, []int{1, 2, 3})
	b = slices.Insert(b, 1, b[0:2]...)
	fmt.Println(b)
	r := []int{1, 2, 3, 4, 5, 6}
	r = slices.Replace(r, 0, 2, r[3:]...)
	fmt.Println(r)

	// iter.Pull / Pull2: next runs the sequence to its next yield, stop ends
	// it, and both may be called again afterwards.
	next, stop := iter.Pull(count(4))
	for {
		v, ok := next()
		if !ok {
			break
		}
		fmt.Print(v, " ")
	}
	_, ok := next()
	fmt.Println(ok)
	stop()
	stop()

	n2, stop2 := iter.Pull(count(100))
	x1, _ := n2()
	x2, _ := n2()
	stop2()
	_, ok3 := n2()
	fmt.Println(x1, x2, ok3)

	p, pstop := iter.Pull2(pairs())
	for {
		k, v, ok := p()
		if !ok {
			break
		}
		fmt.Println(k, v)
	}
	pstop()

	for i := range count(3) {
		fmt.Print(i)
	}
	fmt.Println()
	fmt.Println(slices.Collect(count(5)))
	fmt.Println(slices.Sorted(maps.Keys(map[int]bool{3: true, 1: true})))
}
