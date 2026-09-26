// The Go 1.21 `clear` builtin: a map loses every entry and stays usable; a
// slice keeps its length and capacity with every element set to the element
// type's zero value — including through a sub-slice, which shares its backing
// array, and for struct and array elements, which each get their own zero.
package main

import "fmt"

type pt struct {
	X, Y int
	Tag  string
}

type set map[string]bool

func main() {
	m := map[string]int{"a": 1, "b": 2}
	alias := m
	clear(m)
	fmt.Println(len(m), len(alias), m)
	m["c"] = 3
	fmt.Println(m, alias)

	s := []int{1, 2, 3, 4, 5}
	clear(s[1:3])
	fmt.Println(s, len(s), cap(s))
	clear(s)
	fmt.Println(s)

	strs := []string{"x", "y"}
	clear(strs)
	fmt.Printf("%q\n", strs)

	fs := []float64{1.5, 2.5}
	clear(fs)
	fmt.Println(fs)

	pts := []pt{{1, 2, "a"}, {3, 4, "b"}}
	clear(pts)
	pts[0].X = 9
	fmt.Printf("%+v\n", pts)

	grid := [][2]int{{1, 2}, {3, 4}}
	clear(grid)
	grid[0][1] = 7
	fmt.Println(grid)

	ptrs := []*pt{{X: 1}}
	clear(ptrs)
	fmt.Println(ptrs[0] == nil)

	var nilMap map[string]int
	clear(nilMap)
	var nilSlice []int
	clear(nilSlice)
	fmt.Println(len(nilMap), len(nilSlice))

	seen := set{"k": true}
	clear(seen)
	fmt.Println(len(seen))
}
