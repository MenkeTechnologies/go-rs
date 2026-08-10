// Interface equality. Go decides it by *dynamic type first, value second*: two
// interfaces holding different types are never equal, however well the numbers
// line up. Comparing two interfaces is the only construct in valid Go that puts
// two different types under one operator — arithmetic and ordered comparison on
// mismatched types are compile errors, and an interface is unordered — so this
// file is the whole of the rule's reach.
//
// Only the crossings go-rs can decide at run time are here. `any(97) ==
// any(int64(97))` is still wrong, because every integer width is one 64-bit
// value with no width beside it; that half is BUGS.md's, not this file's.
package main

import (
	"errors"
	"fmt"
)

type pt struct{ a int }

// Same shape and same field values as pt, and a different type — so a
// comparison that rendered both operands and compared the text would call them
// equal, and Go does not.
type qt struct{ a int }

type shape interface{ area() int }

type sq struct{ s int }

func (s sq) area() int { return s.s * s.s }

func main() {
	// The headline: an int and a float64 holding the same number.
	var x any = 1
	var y any = 1.0
	fmt.Println(x == y, x != y)

	// Past 2^53, where promoting the integer to f64 would land on a
	// neighbouring value — the answer is false either way, for the type.
	var p any = int64(1e18)
	var q any = float64(1e18)
	fmt.Println(p == q, p != q)

	// The matched pairs, so a blanket "different" is not a passing answer.
	var z any = 1
	fmt.Println(x == z, x != z, y == y, x == 2)

	// A number beside its own text. Both render as "1"; they are not equal.
	var s any = "1"
	fmt.Println(x == s, s == s, s == "1")

	// A bool beside the string spelling of one.
	var b any = true
	var w any = "true"
	fmt.Println(b == w, b == true, w == "true")

	// An untyped constant takes its default type, so it is compared as an int,
	// a float64 and a string respectively.
	fmt.Println(x == 1, y == 1.0, y == 1, x == 1.0, s == "1")

	// nil. An interface holding a *typed* nil is not a nil interface: the slice
	// and the map each carry the type they were written as, and only the
	// undeclared one is nil. The direct comparisons are true, which is the
	// difference the box makes.
	var none any
	var sl []int
	var mp map[string]int
	var vs any = sl
	var vm any = mp
	fmt.Println(none == nil, x == nil, vs == nil, vm == nil)
	fmt.Println(sl == nil, mp == nil, none == vs, vs != nil)

	// Two struct types with the same field, and the same type twice.
	var g any = pt{1}
	var h any = qt{1}
	var i any = pt{1}
	fmt.Println(g == h, g == i, g != h, g == x)

	// A declared interface, not just `any`, and a nil error beside a real one.
	var sh shape = sq{3}
	var sh2 shape = sq{3}
	var shNil shape
	fmt.Println(sh == sh2, sh == shNil, shNil == nil, sh.area())

	var e error
	fmt.Println(e == nil, e != nil)
	e = errors.New("boom")
	fmt.Println(e == nil, e != nil, e.Error())

	// Two separately allocated errors with the same text are distinct pointers.
	e2 := errors.New("boom")
	e3 := errors.New("boom")
	fmt.Println(e2 == e3, e2 == e2)

	// Through a container and a function boundary, where the static type is
	// gone by the time the comparison runs.
	vals := []any{1, 1.0, "1", true}
	for _, v := range vals {
		fmt.Print(v == 1, " ")
	}
	fmt.Println()
	fmt.Println(same(1, 1.0), same(1, 1), same("1", 1), same(nil, nil))
}

func same(a, b any) bool { return a == b }
