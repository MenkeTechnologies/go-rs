package main

import "fmt"

// `*p` reads the value a pointer addresses: through a `*map[K]V` it is the map
// (`len`, indexing, assignment, `range`, `delete`), and through a pointer to a
// nil slice or map it is that nil, so `*p == nil` holds.

func main() {
	m := map[string]int{"a": 1}
	mp := &m
	fmt.Println(len(*mp))
	fmt.Println((*mp)["a"])
	(*mp)["b"] = 2
	fmt.Println(m, *mp, len(m))
	for k, v := range *mp {
		if k == "a" {
			fmt.Println(k, v)
		}
	}
	delete(*mp, "a")
	fmt.Println(m)
	var e []int
	fmt.Println(*(&e) == nil)
	pe := &e
	fmt.Println(*pe == nil, len(*pe))
	var nm map[string]int
	pnm := &nm
	fmt.Println(*pnm == nil, len(*pnm))
	v, ok := (*mp)["b"]
	fmt.Println(v, ok)
}
