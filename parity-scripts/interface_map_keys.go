package main

// A `map[any]V` compares its keys' *dynamic types* before their values, so `1`
// and `1.0` and `"1"` and `true` are four keys, not one. go-rs used to compare
// every numeric key through `to_float`, which merged the first two — and the
// only reason it could not simply stop was that an untyped constant in a
// `map[float64]V` index arrived as an integer, so the strict rule would have
// lost those lookups instead. Converting the constant at the index site is what
// makes the strict rule safe, and this is where the two meet.
//
// The one distinction go-rs cannot draw is between two *integer widths*: an
// `int` and an `int64` are both an i64 at run time, with the width living in
// the static type an `any` erases. Those cases are left to BUGS.md rather than
// written here, because this file is a byte-parity gate.

import (
	"fmt"
	"sort"
)

type pair struct{ A, B int }

func main() {
	m := map[interface{}]string{}
	m[1] = "int"
	m[1.5] = "float"
	m["1"] = "string"
	m[true] = "bool"
	m[nil] = "nil"
	m[pair{1, 2}] = "struct"
	m[[2]int{1, 2}] = "array"
	fmt.Println("kinds", len(m))
	fmt.Println("read", m[1], m[1.5], m["1"], m[true], m[nil], m[pair{1, 2}], m[[2]int{1, 2}])

	// An integer and a float of the same value are two keys.
	n := map[interface{}]string{}
	n[2] = "int-2"
	n[2.0] = "float-2"
	fmt.Println("int-vs-float", len(n), n[2], n[2.0], n[float64(2)])

	// A float that is not a whole number was never at risk, and still is not.
	fmt.Println("fractional", n[2.5] == "", m[1.5])

	// `false` is not `0`, and `""` is not `0` either.
	z := map[interface{}]string{}
	z[0] = "zero"
	z[false] = "false"
	z[""] = "empty"
	z[0.0] = "zero-float"
	fmt.Println("falsy", len(z), z[0], z[false], z[""], z[0.0])

	// Overwrites still land on the right key.
	z[0] = "zero-again"
	z[false] = "false-again"
	fmt.Println("overwrite", len(z), z[0], z[false], z[0.0])

	// `delete` removes exactly one of them.
	delete(z, 0)
	fmt.Println("delete", len(z), z[0] == "", z[false], z[0.0])

	// Comma-ok reports the right key's presence.
	_, hasInt := z[0]
	_, hasFloat := z[0.0]
	fmt.Println("comma-ok", hasInt, hasFloat)

	// A `map[float64]V` still finds an untyped-constant key, whichever way the
	// constant is written and wherever it comes from.
	f := map[float64]int{1: 10, 2.5: 20, 3: 30}
	f[4] = 40
	fmt.Println("float-map", len(f), f[1], f[1.0], f[float64(1)], f[2.5], f[3], f[4])

	// And a `map[int]V` is unaffected by any of it.
	i := map[int]int{1: 10}
	i[2] = 20
	fmt.Println("int-map", len(i), i[1], i[2], i[3])

	// Iteration yields every key of the mixed map exactly once.
	var got []string
	for k := range m {
		got = append(got, fmt.Sprintf("%v", k))
	}
	sort.Strings(got)
	fmt.Println("iterate", len(got), got)

	// A `map[any]V` used as a set of mixed kinds.
	seen := map[interface{}]bool{}
	for _, k := range []interface{}{1, 1.0, "1", true, nil} {
		seen[k] = true
	}
	fmt.Println("set", len(seen), seen[1], seen[1.0], seen["1"], seen[true], seen[nil], seen[2])
}
