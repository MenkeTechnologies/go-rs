// The three-index (full) slice expression s[low:high:max]: it caps the result's
// capacity at max-low, which is what stops a later append from writing into
// backing array the sub-slice no longer owns.
package main

import "fmt"

func main() {
	base := make([]int, 5, 5)
	fmt.Println(len(base[1:2:3]), cap(base[1:2:3]))

	b := make([]int, 3, 10)
	fmt.Println(len(b), cap(b), len(b[1:2]), cap(b[1:2]))

	// A capped view has room for one more element inside the shared backing.
	c := b[1:2:5]
	fmt.Println(len(c), cap(c))
	d := append(c, 99)
	fmt.Println(len(d), cap(d), d[0], d[1], b[2])

	// Appending past the cap reallocates, leaving the parent untouched.
	e := append(c, 1, 2, 3, 4)
	fmt.Println(len(e), cap(e), b[2])

	// s[a:b:b] is the idiom that forces the next append to copy.
	f := b[0:3:3]
	g := append(f, 7)
	fmt.Println(len(g), cap(g), b[0], b[1], b[2], len(b))

	// Re-slicing a capped view is bounded by the view's cap, not the backing's.
	h := b[0:1:2]
	fmt.Println(len(h[:2]), cap(h[:2]))

	// The bounds are ordinary expressions.
	i, j, k := 1, 3, 4
	fmt.Println(len(base[i:j:k]), cap(base[i:j:k]))

	// Capacity survives through a copy of the header.
	m := b[0:2:4]
	n := m
	fmt.Println(cap(n), len(n))
}
