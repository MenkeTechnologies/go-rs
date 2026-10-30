package main

import (
	"fmt"
	"strings"
)

// A missing key of a defined map type (`type Set map[string]bool`) reads its
// base's value zero, in a method on the type and through comma-ok alike.
type Set map[string]bool
type Counts map[string]int
type Names []string
type Index map[string][]int

func (n Names) Len() int { return len(n) }

func main() {
	s := Set{"a": true}
	fmt.Println(s["a"], s["b"])
	v, ok := s["b"]
	fmt.Println(v, ok)
	c := Counts{}
	c["x"]++
	fmt.Println(c["x"], c["y"])
	idx := Index{}
	idx["k"] = append(idx["k"], 1)
	fmt.Println(idx["k"], idx["missing"] == nil, len(idx["missing"]))
	var ns Names
	ns = append(ns, "z")
	fmt.Println(ns, ns.Len())
	type M map[int]string
	m := M{1: "one"}
	fmt.Printf("%q %q\n", m[1], m[2])
	nested := map[string]map[string]int{}
	if nested["a"] == nil {
		nested["a"] = map[string]int{}
	}
	nested["a"]["b"]++
	fmt.Println(nested, nested["zz"]["q"])
	fs := map[string]float64{}
	fmt.Println(fs["q"] + 0.5)
	ss := map[int]string{}
	fmt.Println(strings.ToUpper(ss[3]) == "", len(ss[3]))
	type P struct{ X int }
	ps := map[string]P{}
	fmt.Println(ps["n"].X, ps["n"])
	pp := map[string]*P{}
	fmt.Println(pp["n"] == nil)
}
