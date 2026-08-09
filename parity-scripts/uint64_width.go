// Unsigned 64-bit integers. `uint64`, `uint` and `uintptr` share `int64`'s
// two's-complement bit pattern, so `+ - * << & | ^ &^` need nothing special —
// but every operation that reads the sign bit (`/`, `%`, `>>`, the ordered
// comparisons, the conversion to a float, and printing) must be done unsigned.
package main

import "fmt"

type box struct {
	u uint64
	n int
}

func half(x uint64) uint64 { return x / 2 }

func main() {
	// Underflow wraps to the top of the range, and prints as such.
	var z uint64 = 0
	z--
	fmt.Println(z)
	var u uint = 0
	u--
	fmt.Println(u)

	var x uint64 = 1 << 63
	fmt.Println(x, x > 100, x/3, x%7, x>>1)
	fmt.Printf("%d|%v|%x|%X|%o|%b|%T\n", x, x, x, x, x, x, x)

	// Through a declared result type.
	fmt.Println(half(x))

	// Struct fields, slices and maps carry the width into `fmt` too.
	b := box{u: x, n: -1}
	fmt.Println(b)
	fmt.Printf("%v %+v\n", b, b)
	fmt.Println(b.u, b.u/4, b.u > 1000)
	fmt.Println([]uint64{x, 1, x / 3})
	fmt.Println(map[string]uint64{"a": x})

	// Compound assignment picks the width up from the target.
	var c uint64 = 10
	c -= 20
	fmt.Println(c)
	c /= 3
	fmt.Println(c)
	var d uint64 = 1 << 63
	d >>= 4
	fmt.Println(d)
	s := []uint64{x, 40, 2}
	s[1] -= 50
	fmt.Println(s[1])

	// `==` is signedness-blind: equal bit patterns are equal either way.
	var e uint64 = 18446744073709551615
	fmt.Println(e == x, e != x, e == 18446744073709551615)
	var lo uint64 = 1
	fmt.Println(lo < x, x < lo, lo <= lo, x >= e)

	// `uint` and `uintptr` behave identically; `int` stays signed.
	var w uint = 1 << 63
	fmt.Println(w, w/5, w>>2, w > 7)
	var p uintptr = 1 << 63
	fmt.Println(p, p/2)
	var i int = -8
	fmt.Println(i, i/3, i>>1, i < 0)

	// Widening to a float reads the unsigned value.
	fmt.Println(int64(x))
	fmt.Println(float64(x))
	fmt.Println(uint64(float64(1e18)))

	// A shift takes its type from the left operand alone, so a `uint` count
	// does not make a signed shift logical.
	var sh uint = 3
	var g int8 = -128
	var h int = -64
	fmt.Println(g>>sh, h>>sh)

	var acc uint64
	for k := 0; k < 3; k++ {
		acc += x / 4
	}
	fmt.Println(acc)
}
