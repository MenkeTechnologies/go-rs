// `float32` is a real 32-bit type: every operation rounds at 32 bits (rounding
// an f64 result afterwards rounds twice and lands a ulp away), and `fmt` prints
// the shortest decimal that round-trips *at 32 bits*.
package main

import "fmt"

type vec struct {
	x float32
	y float64
}

func half(f float32) float32 { return f / 2 }

func main() {
	var f float32 = 1.0 / 3.0
	fmt.Println(f)

	// Rounded once, not twice: in f64 this product is 3.3554434e+06.
	var a float32 = 16777217.0
	var b float32 = 0.2
	fmt.Println(a * b)

	var c float32 = 0.1
	fmt.Println(c, c+c, c*3, c-0.05, c/3)

	fmt.Printf("%v %g %e %f %.2f %T\n", f, f, f, f, f, f)
	fmt.Printf("%.3f %8.4f| %G\n", f, f, f)

	// The f64 with the same bits prints differently.
	var d float64 = 1.0 / 3.0
	fmt.Println(d, float32(d), float64(float32(d)))

	// Slices, maps and struct fields all render element-wise at 32 bits.
	xs := []float32{1.0 / 3.0, 0.1, 2.0 / 7.0}
	fmt.Println(xs, xs[0], xs[0]*3)
	m := map[string]float32{"a": 1.0 / 3.0}
	fmt.Println(m["a"], m)
	v := vec{x: 1.0 / 3.0, y: 1.0 / 3.0}
	fmt.Println(v.x, v.y)
	fmt.Printf("%v %+v\n", v, v)
	fmt.Println([]vec{v})

	// Results, compound assignment and loops keep the width.
	fmt.Println(half(1.0))
	var g float32 = 0.1
	g += 0.2
	fmt.Println(g)
	var h float32 = 1
	for i := 0; i < 5; i++ {
		h = h / 3
	}
	fmt.Println(h)

	// An untyped constant beside a float32 is a float32, in comparisons too.
	fmt.Println(float32(0.1) == 0.1, float32(2) < float32(3), f > 0.3)

	// Edges: the exponent window, the extremes, and the non-finite cases.
	var big float32 = 3.4e38
	var tiny float32 = 1e-45
	fmt.Println(big, big*2, tiny, -tiny)
	fmt.Println(float32(999999), float32(1000000), float32(0.0001), float32(0.00001))
	z := float32(0)
	fmt.Println(z, -z, float32(1)/float32(3))
	// 4025693.25 is exactly between two 8-digit decimals: Go rounds to even.
	fmt.Println(float32(4025693.25), float32(-141491.625))
}
