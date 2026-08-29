package main

import "fmt"

func main() {
	// append may or may not share the backing array; cap growth is what
	// decides, and a 3-index slice caps it explicitly.
	a := []int{1, 2, 3, 4, 5}
	b := a[1:3]
	fmt.Println(b, len(b), cap(b))
	c := a[1:3:3]
	fmt.Println(c, len(c), cap(c))
	b = append(b, 99)
	fmt.Println(a, b)
	c = append(c, 77)
	fmt.Println(a, c)
	// copy returns how many it moved: the shorter of the two.
	dst := make([]int, 2)
	fmt.Println(copy(dst, a), dst)
	// A nil slice and an empty slice differ on == nil but not on len/append.
	var nilS []int
	empty := []int{}
	fmt.Println(nilS == nil, empty == nil, len(nilS), len(empty))
	fmt.Println(append(nilS, 1), append(empty, 1))
	// Map: absent key gives the zero value, and the comma-ok form says which.
	m := map[string]int{"a": 1}
	v, ok := m["zz"]
	fmt.Println(v, ok)
	m["b"] = 2
	delete(m, "a")
	fmt.Println(len(m), m["b"])
	// Struct equality is field-wise.
	type P struct{ X, Y int }
	fmt.Println(P{1, 2} == P{1, 2}, P{1, 2} == P{2, 1})
	fmt.Println(fmt.Sprintf("%v %+v", P{1, 2}, P{1, 2}))
}
