// A defined type — `type Weekday int` — is a distinct type in Go carrying its
// base's representation. Everything it does at run time is the base's; the one
// thing that is not is the name, which `%T` prints, `%#v` writes on a composite
// literal, and a method declared on it is reached through. Both halves are
// printed for each base kind: dropping the name and treating the type as opaque
// are both wrong, and only one of the two shows up in a `%T`-only check.
package main

import "fmt"

type myInt int
type myStr string
type myFloat float64
type myBool bool
type mySlice []int
type myMap map[string]int
type myArr [3]int
type myFunc func(int) int
type myChan chan int

func (m myInt) triple() myInt { return m * 3 }

func bump(m myInt) myInt { return m + 1 }

func main() {
	n := myInt(7)
	fmt.Printf("%T %v %d %q\n", n, n, n, n)
	fmt.Printf("%T %T %T\n", n+1, n*2, -n)
	fmt.Printf("%T %v %T %v\n", n.triple(), n.triple(), bump(n), bump(n))
	fmt.Printf("%T %v\n", int(n), int(n)+1)

	var s myStr = "hi"
	fmt.Printf("%T %q %s %v %v\n", s, s, s, s+"!", len(s))

	f := myFloat(1.5)
	fmt.Printf("%T %v %.2f %T\n", f, f, f, f*2)

	b := myBool(true)
	fmt.Printf("%T %v %t\n", b, b, !b)

	sl := mySlice{3, 1, 2}
	fmt.Printf("%T %v %d %#v %v %v\n", sl, sl, sl, sl, len(sl), sl[0])
	sl = append(sl, 4)
	fmt.Printf("%T %v\n", sl, sl)

	m := myMap{"a": 1}
	fmt.Printf("%T %v %d %#v %v\n", m, m, m, m, m["a"])

	a := myArr{1, 2, 3}
	fmt.Printf("%T %v %d\n", a, a, a)

	// A zero value is the base's, so a defined slice or map is nil-but-usable
	// and prints as the empty composite rather than as `<nil>`.
	var zs mySlice
	var zm myMap
	var zf myFunc
	var zc myChan
	fmt.Printf("%T %v %T %v\n", zs, zs, zm, zm)
	fmt.Printf("%T %T %v %v\n", zf, zc, zf == nil, zc == nil)
	fmt.Printf("%v %v\n", len(zs), len(zm))

	// The name reaches into a container's element and key types too.
	fmt.Printf("%T %v\n", []myInt{1, 2}, []myInt{1, 2})
	fmt.Printf("%T\n", map[myStr]myInt{"k": 1})
	fmt.Printf("%T %v\n", [2]myStr{"x", "y"}, [2]myStr{"x", "y"})

	// It survives a plain copy and an equality test, both of which are the
	// base's behaviour.
	n2, s2 := n, s
	fmt.Printf("%T %T %v %v\n", n2, s2, n2 == n, s2 == s)
}
