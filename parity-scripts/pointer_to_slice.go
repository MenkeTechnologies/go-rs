package main

import "fmt"

// A `*[]T` is a pointer to a slice, and every read through it answers about the
// slice it addresses: `len`, `cap`, indexing, `range`, and `append`'s operand.

func total(p *[]int) int {
	sum := 0
	for _, v := range *p {
		sum += v
	}
	return sum
}

func firstTwo(p *[]string) (string, string) { return (*p)[0], (*p)[1] }

func main() {
	s := []int{1, 2, 3}
	p := &s
	fmt.Println(len(*p), cap(*p) >= 3, (*p)[1], *p)
	fmt.Println(total(&s))
	fmt.Println(total(p))

	// `append`'s first operand read through the pointer.
	grown := append(*p, 4)
	fmt.Println(grown, len(grown))

	// A write through the pointer is seen by the variable.
	(*p)[0] = 9
	fmt.Println(s, (*p)[0])

	// A nil slice behind a pointer reads as empty rather than faulting. (`*ep ==
	// nil` is still false — BUGS.md.)
	var empty []int
	ep := &empty
	fmt.Println(len(*ep), cap(*ep))

	// A pointer to a slice of a non-numeric element type.
	words := []string{"alpha", "beta"}
	fmt.Println(firstTwo(&words))

	// A sub-slice view behind a pointer keeps its own length and capacity.
	view := s[1:2]
	vp := &view
	fmt.Println(len(*vp), cap(*vp), (*vp)[0])

	// Rebinding the variable still leaves an earlier copy alone.
	t := s
	s = append(s, 7)
	fmt.Println(t, s)
}
