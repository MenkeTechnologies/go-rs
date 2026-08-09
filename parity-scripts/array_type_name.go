// `%T` and `%#v` read the *value*, and in go-rs a `[N]T` and a `[]T` are the
// same heap object — so the array's length, which is part of its type, has to
// be carried on the object itself. It is stamped where an array is born (a
// composite literal, a zero value, a `var` declaration) and copied along with
// the array at every site Go copies one, which is why the sections below check
// the name after an assignment, a parameter bind, a return and an `any` box as
// well as at the literal. The slice sections are the control: the same
// elements spelled `[]T` must still name a slice.
package main

import "fmt"

type pt struct{ x, y int }

func through(v any) string { return fmt.Sprintf("%T", v) }

func ret() [2]int { return [2]int{7, 8} }

func bind(a [3]string) string { return fmt.Sprintf("%T", a) }

type holder struct {
	a [2]int
	s []int
	n [2][3]int
}

func main() {
	// A literal names its length; the same elements as a slice do not.
	fmt.Printf("%T %T\n", [3]int{1, 2, 3}, []int{1, 2, 3})
	fmt.Printf("%T %T\n", [2]string{"a", "b"}, []string{"a", "b"})

	// Nesting is part of the name, and an array of slices differs from a
	// slice of arrays in both positions.
	fmt.Printf("%T\n", [2][3]int{{1, 2, 3}, {4, 5, 6}})
	fmt.Printf("%T\n", [2][]int{{1}, {2}})
	fmt.Printf("%T\n", [][2]int{{1, 2}})

	// A declared type is package-qualified inside the name.
	fmt.Printf("%T %T\n", [2]pt{{1, 2}, {3, 4}}, []pt{{1, 2}})
	fmt.Printf("%T\n", [2]map[string]pt{})
	fmt.Printf("%T\n", [2]*pt{})

	// Zero values: `var`, the empty literal, and an element beyond the given
	// ones all produce a named array.
	var z [4]bool
	fmt.Printf("%T %v\n", z, z)
	var multi, other [2]int
	fmt.Printf("%T %T\n", multi, other)
	fmt.Printf("%T %v\n", [0]int{}, [0]int{})
	fmt.Printf("%T\n", [3]float64{1})

	// The name survives every site Go copies an array.
	a := [3]int{1, 2, 3}
	b := a
	fmt.Printf("%T %T\n", a, b)
	fmt.Println(bind([3]string{"x", "y", "z"}))
	fmt.Printf("%T\n", ret())
	fmt.Println(through(a), through([]int{1, 2, 3}))
	var i any = [2][3]int{}
	fmt.Printf("%T\n", i)

	// …including out of a container, where the read is itself a copy.
	h := holder{a: [2]int{1, 2}, s: []int{3}, n: [2][3]int{}}
	fmt.Printf("%T %T %T\n", h.a, h.s, h.n)
	m := map[string][2]int{"k": {1, 2}}
	fmt.Printf("%T\n", m["k"])
	xs := [][2]int{{1, 2}, {3, 4}}
	fmt.Printf("%T %T\n", xs, xs[0])
	for _, e := range xs {
		fmt.Printf("%T ", e)
	}
	fmt.Println()

	// `%#v` writes the same name, at every depth.
	fmt.Printf("%#v\n", [3]int{1, 2, 3})
	fmt.Printf("%#v\n", []int{1, 2, 3})
	fmt.Printf("%#v\n", [2][3]int{{1, 2, 3}, {4, 5, 6}})
	fmt.Printf("%#v\n", [2]pt{{1, 2}, {3, 4}})
	fmt.Printf("%#v\n", [2][]int{{1}, {2}})

	// `%v` is unaffected by the name — an array and a slice print alike.
	fmt.Println([3]int{1, 2, 3}, []int{1, 2, 3})
	fmt.Printf("%v %+v\n", [2]pt{{1, 2}, {3, 4}}, [2]pt{{1, 2}, {3, 4}})

	// A slice *of* an array is a slice again, and an append off one too.
	sl := a[:]
	fmt.Printf("%T %T\n", sl, append(sl, 4))

	// Width-tagged element types keep the name (the `fmt` box rebuilds the
	// container on its way in).
	var f [2]float32
	var u [2]uint64
	fmt.Printf("%T %T\n", f, u)

	// A type switch still selects on the element shape, unaffected.
	switch any(a).(type) {
	case [3]int:
		fmt.Println("switch [3]int")
	default:
		fmt.Println("switch default")
	}
}
