package main

// A composite literal inside another one may drop the element type. The parser
// only has the element type's *text*, so the two forms where that text is not a
// struct name are resolved in the compiler, where the type tables are complete:
// a pointer element (`[]*T{{…}}` means `&T{…}`, and the result has to carry the
// same pointer mark the written form does) and a defined type over a slice or
// array (`type ints []int` elides to its base's literal, not to a struct).

import "fmt"

type pt struct{ X, Y int }
type ints []int
type grid [2]int

func main() {
	ps := []*pt{{1, 2}, {3, 4}}
	ps[0].X = 9
	fmt.Println(ps[0].X, ps[0].Y, ps[1].X, len(ps))

	m := map[string]*pt{"a": {5, 6}}
	m["a"].Y = 7
	fmt.Println(m["a"].X, m["a"].Y)

	d := map[string]ints{"a": {1, 2}}
	fmt.Println(d["a"], len(d["a"]))

	sl := []ints{{1}, {2, 3}}
	fmt.Println(sl[0], sl[1], len(sl))

	g := []grid{{1, 2}, {3, 4}}
	fmt.Println(g[0], g[1])

	// values still work
	vs := []pt{{1, 2}}
	fmt.Println(vs[0].X)

	// A pointer element elided inside a nested literal keeps sharing.
	nested := [][]*pt{{{8, 9}}}
	alias := nested[0][0]
	alias.X = 10
	fmt.Println(nested[0][0].X, alias.X)

	// An elided struct element is still a value, so it copies.
	vals := []pt{{1, 2}}
	got := vals[0]
	got.X = 99
	fmt.Println(vals[0].X, got.X)

	// A written-out form and an elided one build the same thing.
	a := []*pt{{1, 2}}
	b := []*pt{&pt{1, 2}}
	fmt.Println(a[0].X == b[0].X, a[0] == b[0])
}
