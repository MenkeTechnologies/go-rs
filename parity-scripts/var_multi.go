// A `var` declaration binds a whole name list: with a type, without one, with
// an initializer list, with a single multi-value call, grouped, and at package
// level.
package main

import "fmt"

var gx, gy int = 7, 8

var (
	ga, gb     = "a", "b"
	gc, gd, ge float64
	gf, gg, gh = 1, 2.5, "three"
)

func two() (int, string) { return 3, "z" }

func widths() (int8, uint8) { return -1, 200 }

func main() {
	var a, b int = 1, 2
	var c, d = 10, "s"
	var e, f int
	var g, h = two()
	var i, j string
	var k, l bool
	fmt.Println(a, b, c, d, e, f, g, h)
	fmt.Printf("%q %q %v %v\n", i, j, k, l)
	fmt.Println(gx, gy, ga, gb, gc, gd, ge, gf, gg, gh)

	// The written type still drives fixed-width wrapping.
	var wide, wider int = 300, 5000000000
	fmt.Println(wide, wider)
	var s8, t8 int8 = 100, 100
	fmt.Println(s8+t8, s8*2)
	var u8, v8 uint8 = 200, 100
	fmt.Println(u8 + v8)
	var p8, q8 = widths()
	fmt.Println(p8, q8)

	// Slices and maps declared together each get their own typed nil.
	var xs, ys []int
	var m1, m2 map[string]int
	fmt.Println(xs, ys, m1, m2, xs == nil, m2 == nil)
	xs = append(xs, 1)
	fmt.Println(xs, ys)
}
