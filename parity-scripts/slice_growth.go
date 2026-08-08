package main

// Go's slice header is (pointer, len, cap): `make` can reserve spare room,
// `append` grows the backing array by runtime.nextslicecap when it is full and
// otherwise writes in place, and a re-slice is bounded by cap rather than len.
import "fmt"

func main() {
	// append grows capacity by doubling below the 256-element threshold.
	var s []int
	for i := 0; i < 20; i++ {
		s = append(s, i)
		fmt.Print(len(s), ":", cap(s), " ")
	}
	fmt.Println()
	fmt.Println(s)

	// make reserves capacity without extending the length.
	m := make([]int, 3, 10)
	fmt.Println(len(m), cap(m), m)

	// An append that fits in the spare room writes into the same backing array,
	// so re-slicing past len (legal up to cap) sees it.
	n := append(m, 7)
	fmt.Println(len(n), cap(n), n, m[0:4], m[:cap(m)])

	// A full slice reallocates, so the result does not alias the original.
	a := []int{1, 2, 3}
	b := append(a, 4)
	b[0] = 99
	fmt.Println(a, b, len(a), cap(a), len(b), cap(b))

	// A sub-slice shares the parent's backing array.
	base := make([]int, 5, 5)
	for i := range base {
		base[i] = i
	}
	sub := base[1:3]
	fmt.Println(sub, len(sub), cap(sub))
	sub[0] = 42
	fmt.Println(base, sub)

	// Appending many elements at once. Only the length is asserted here: Go
	// rounds the new backing array up to a malloc size class, which needs the
	// element size go-rs does not carry (see BUGS.md), so cap can read low.
	var big []int
	big = append(big, 1, 2, 3, 4, 5)
	big = append(big, []int{6, 7}...)
	fmt.Println(len(big), big)

	// copy reports how many elements moved.
	dst := make([]int, 2)
	fmt.Println(copy(dst, []int{8, 9, 10}), dst)

	// make without a capacity gives cap == len.
	fmt.Println(len(make([]int, 4)), cap(make([]int, 4)))
}
