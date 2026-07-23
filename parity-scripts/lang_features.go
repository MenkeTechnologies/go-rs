// Assorted language features: fmt.Sprintf/Sprint, slice expressions on slices
// and strings, address-of & pointer receivers, switch (tagged, tagless,
// multi-expr, init, break/continue), calling a function value from an index,
// and named return values (bare return, deferred mutation, recover).
package main

import "fmt"

type Node struct {
	val  int
	next *Node
}

func (n *Node) sum() int {
	total := 0
	for cur := n; cur != nil; cur = cur.next {
		total += cur.val
	}
	return total
}

func classify(n int) string {
	switch {
	case n < 0:
		return "negative"
	case n == 0:
		return "zero"
	case n < 10:
		return "small"
	default:
		return "large"
	}
}

func label(day int) string {
	switch day {
	case 0, 6:
		return "weekend"
	case 1, 2, 3, 4, 5:
		return "weekday"
	default:
		return "invalid"
	}
}

func minmax(xs []int) (lo, hi int) {
	lo, hi = xs[0], xs[0]
	for _, v := range xs {
		if v < lo {
			lo = v
		}
		if v > hi {
			hi = v
		}
	}
	return
}

func main() {
	// Sprintf / Sprint.
	msg := fmt.Sprintf("[%d:%s]", 7, "go")
	fmt.Println(msg, len(msg))
	fmt.Println(fmt.Sprint("x", 1, 2, "y"))

	// Slice expressions.
	xs := []int{5, 4, 3, 2, 1}
	fmt.Println(xs[1:4], xs[:2], xs[3:])
	s := "abcdefg"
	fmt.Println(s[2:5], s[:3], s[4:])

	// Address-of and a linked list via pointers.
	a := &Node{val: 1}
	a.next = &Node{val: 2}
	a.next.next = &Node{val: 3}
	fmt.Println(a.sum())

	// switch.
	fmt.Println(classify(-3), classify(0), classify(7), classify(42))
	fmt.Println(label(0), label(3), label(9))

	// break/continue interaction with an enclosing loop.
	acc := 0
	for i := 0; i < 6; i++ {
		switch i {
		case 3:
			continue
		case 5:
			break
		}
		acc += i
	}
	fmt.Println(acc)

	// Function values in a slice, called by index.
	fns := []func(int) int{
		func(x int) int { return x + 1 },
		func(x int) int { return x * x },
	}
	fmt.Println(fns[0](10), fns[1](10))

	// Named results.
	lo, hi := minmax([]int{7, 2, 9, 4, 1, 8})
	fmt.Println(lo, hi)
}
