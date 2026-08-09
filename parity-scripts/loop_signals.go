// Loop signals: `break` and `continue`, labelled and unlabelled, across all
// three `for` forms and `range`, plus `switch` fallthrough and the `break` that
// leaves a `switch` rather than its enclosing loop.
package main

import "fmt"

func main() {
outer:
	for i := 0; i < 4; i++ {
		for j := 0; j < 4; j++ {
			if j == 2 {
				continue outer
			}
			if i == 3 {
				break outer
			}
			fmt.Println(i, j)
		}
	}

	// Infinite form.
	k := 0
	for {
		k++
		if k%2 == 0 {
			continue
		}
		if k > 7 {
			break
		}
		fmt.Println("k", k)
	}

	// Condition-only form.
	m := 0
	for m < 5 {
		m++
		if m == 3 {
			continue
		}
		fmt.Println("m", m)
	}

	// range over a slice.
	for i, v := range []int{10, 20, 30, 40} {
		if v == 20 {
			continue
		}
		if i == 3 {
			break
		}
		fmt.Println("r", i, v)
	}

	// range over a map is unordered in Go, so only its length is asserted.
	mp := map[string]int{"a": 1, "b": 2, "c": 3}
	seen := 0
	for range mp {
		seen++
	}
	fmt.Println("seen", seen)

	// range over a string yields byte offsets and runes.
	for i, r := range "héllo" {
		if r == 'l' {
			continue
		}
		fmt.Println("s", i, string(r))
	}

	// fallthrough chains into the next case body unconditionally.
	for x := 0; x < 4; x++ {
		switch x {
		case 0:
			fmt.Println("zero")
			fallthrough
		case 1:
			fmt.Println("one")
		case 2:
			fmt.Println("two")
			fallthrough
		default:
			fmt.Println("def")
		}
	}

	// A bare `break` inside a switch leaves the switch, not the loop.
	for x := 0; x < 3; x++ {
		switch x {
		case 1:
			break
		default:
			fmt.Println("sw", x)
		}
		fmt.Println("after", x)
	}

	// A labelled break may leave a labelled switch.
sw:
	switch 2 {
	case 2:
		for n := 0; n < 5; n++ {
			if n == 2 {
				break sw
			}
			fmt.Println("n", n)
		}
		fmt.Println("unreached")
	}

	// Three loops deep, breaking the middle one.
	for a := 0; a < 2; a++ {
	mid:
		for b := 0; b < 3; b++ {
			for c := 0; c < 3; c++ {
				if c == 1 {
					continue mid
				}
				if b == 2 {
					break mid
				}
				fmt.Println("abc", a, b, c)
			}
		}
	}
}
