// The zero value of a slice or map is a *typed* nil: it prints as `[]` / `map[]`
// (not `<nil>`), reads like an empty one, is appendable, and still `== nil`.
// Writing to a nil map is the one operation that faults.
package main

import "fmt"

type box struct {
	xs []int
	m  map[string]int
	n  int
}

func zero() ([]int, map[string]int) { return nil, nil }

func main() {
	var s []int
	var m map[string]int
	fmt.Println(s, m)
	fmt.Printf("%v|%+v|%#v|%T\n", s, s, s, s)
	fmt.Printf("%v|%+v|%#v|%T\n", m, m, m, m)
	fmt.Println(s == nil, m == nil, s != nil)

	// Reads on a nil slice/map are the empty-container answers.
	fmt.Println(len(s), cap(s), len(m), m["absent"])
	v, ok := m["absent"]
	fmt.Println(v, ok)
	delete(m, "absent")
	for range s {
		fmt.Println("never")
	}
	for k := range m {
		fmt.Println(k)
	}

	// `append` to a nil slice allocates, exactly as Go's does.
	s = append(s, 1, 2)
	fmt.Println(s, len(s), cap(s))

	// A nil `[]byte` is the empty string for the text verbs.
	var bs []byte
	fmt.Printf("%s|%q|%d\n", bs, bs, bs)

	// Struct fields, results and explicit `nil`s carry the same typed zero.
	var b box
	fmt.Println(b, b.xs == nil, b.m == nil, len(b.xs))
	a, c := zero()
	fmt.Println(a, c)
	var d []string = nil
	var e map[int]bool = nil
	fmt.Println(d, e, d == nil, e == nil)

	// Only a *write* to a nil map faults, with Go's un-prefixed message.
	defer func() { fmt.Println("recovered:", recover()) }()
	m["k"] = 1
	fmt.Println("unreachable")
}
