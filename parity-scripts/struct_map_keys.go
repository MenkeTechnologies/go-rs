package main

// Struct and array map keys compare by value — two separately built keys with
// the same fields are the same key — and they hash by value too, so a map of
// them is linear to build rather than quadratic. The hash projection has to
// partition keys exactly the way the equality does: anything it separates that
// equality calls equal is an entry no lookup can reach again, and anything it
// merges is an overwrite that should have been an insert.
//
// The cases below are the ones where those two rules can disagree: keys that
// differ only in one nested field, keys of unlike struct types with identical
// fields, an array against a struct holding the same numbers, and the mixed-kind
// keys of a `map[any]V`.

import (
	"fmt"
	"sort"
)

type pair struct {
	A, B int
}

type samePair struct {
	A, B int
}

type nested struct {
	P    pair
	Name string
}

type withPtr struct {
	N int
	P *int
}

func main() {
	// Distinct keys stay distinct, and a re-built key finds its entry.
	m := map[pair]string{}
	m[pair{1, 2}] = "a"
	m[pair{2, 1}] = "b"
	m[pair{1, 2}] = "c"
	fmt.Println("basic", len(m), m[pair{1, 2}], m[pair{2, 1}], m[pair{3, 3}] == "")

	// Two struct types with identical fields are different types, so their
	// values are never the same key of a `map[any]V`.
	any1 := map[interface{}]string{}
	any1[pair{1, 2}] = "pair"
	any1[samePair{1, 2}] = "samePair"
	fmt.Println("types", len(any1), any1[pair{1, 2}], any1[samePair{1, 2}])

	// An array key is not a struct key, whatever the numbers are.
	any2 := map[interface{}]string{}
	any2[[2]int{1, 2}] = "array"
	any2[pair{1, 2}] = "struct"
	fmt.Println("array-vs-struct", len(any2), any2[[2]int{1, 2}], any2[pair{1, 2}])

	// Nested structs compare and hash field by field, all the way down.
	n := map[nested]int{}
	n[nested{pair{1, 2}, "x"}] = 1
	n[nested{pair{1, 2}, "y"}] = 2
	n[nested{pair{1, 3}, "x"}] = 3
	n[nested{pair{1, 2}, "x"}] += 10
	fmt.Println("nested", len(n), n[nested{pair{1, 2}, "x"}], n[nested{pair{1, 3}, "x"}])

	// Multi-dimensional array keys.
	a := map[[2][2]int]string{}
	a[[2][2]int{{1, 2}, {3, 4}}] = "first"
	a[[2][2]int{{1, 2}, {3, 5}}] = "second"
	fmt.Println("array2d", len(a), a[[2][2]int{{1, 2}, {3, 4}}], a[[2][2]int{{9, 9}, {9, 9}}] == "")

	// A string field, a bool field and a float field all take part.
	type mixed struct {
		S string
		B bool
		F float64
	}
	mx := map[mixed]int{}
	mx[mixed{"k", true, 1.5}] = 1
	mx[mixed{"k", false, 1.5}] = 2
	mx[mixed{"k", true, 2.5}] = 3
	mx[mixed{"k", true, 1.5}] = 4
	fmt.Println("mixed", len(mx), mx[mixed{"k", true, 1.5}])

	// A zero-field struct is one key, so every value of it collides.
	type unit struct{}
	u := map[unit]int{}
	u[unit{}] = 1
	u[unit{}] = 2
	fmt.Println("unit", len(u), u[unit{}])

	// Two keys sharing the same pointer are the same key, and a nil pointer
	// field is a key of its own. (Two *distinct* pointers to equal values are
	// deliberately not probed: go-rs models `&x` on a scalar as the value, so
	// it has no address to compare — see BUGS.md.)
	p1 := 1
	wp := map[withPtr]string{}
	wp[withPtr{1, &p1}] = "p1"
	wp[withPtr{1, &p1}] = "p1-again"
	wp[withPtr{1, nil}] = "nil"
	fmt.Println("ptrfield", len(wp), wp[withPtr{1, &p1}], wp[withPtr{1, nil}])

	// `delete` must find a value-equal key and drop exactly that entry, and the
	// remaining entries must still be reachable afterwards.
	d := map[pair]int{}
	for i := 0; i < 12; i++ {
		d[pair{i, i * 2}] = i
	}
	delete(d, pair{5, 10})
	delete(d, pair{99, 99})
	fmt.Println("delete", len(d), d[pair{5, 10}], d[pair{6, 12}], d[pair{11, 22}])

	// Iteration still yields every key exactly once after the deletes.
	var got []string
	for k, v := range d {
		got = append(got, fmt.Sprintf("%d/%d/%d", k.A, k.B, v))
	}
	sort.Strings(got)
	fmt.Println("iterate", len(got), got[0], got[len(got)-1])

	// Past the point where the map switches from a scan to an index, every key
	// inserted before the switch must still be found.
	big := map[pair]int{}
	for i := 0; i < 200; i++ {
		big[pair{i % 20, i / 20}] = i
	}
	sum := 0
	for i := 0; i < 200; i++ {
		sum += big[pair{i % 20, i / 20}]
	}
	fmt.Println("grown", len(big), sum, big[pair{0, 0}], big[pair{19, 9}])

	// Overwrites keep first-mention order, so `fmt`'s sorted map printing is
	// unaffected by how many times a key was written.
	ord := map[pair]int{}
	ord[pair{2, 2}] = 1
	ord[pair{1, 1}] = 2
	ord[pair{2, 2}] = 3
	fmt.Println("order", ord)

	// A struct key used as a set, with the comma-ok membership test.
	seen := map[pair]bool{}
	seen[pair{7, 8}] = true
	_, in := seen[pair{7, 8}]
	_, out := seen[pair{8, 7}]
	fmt.Println("set", in, out, seen[pair{8, 7}])

	// A struct value read out of a map is a copy: mutating it leaves the key's
	// entry alone, and the key itself still finds that entry.
	store := map[pair]nested{}
	store[pair{1, 1}] = nested{pair{5, 5}, "orig"}
	got2 := store[pair{1, 1}]
	got2.Name = "changed"
	fmt.Println("copy", store[pair{1, 1}].Name, got2.Name)
}
