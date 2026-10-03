package main

import (
	"fmt"
	"math"
)

func main() {
	for _, x := range []float64{3.75, -2.5, 0, math.Inf(1), 1e300, 5e-324} {
		i, f := math.Modf(x)
		fr, ex := math.Frexp(x)
		fmt.Println(x, i, f, fr, ex, math.Ldexp(fr, ex))
	}
	fmt.Println(math.Ldexp(0.5, 4), math.Ldexp(1, -1074), math.Ldexp(1, 1024), math.Ldexp(1, -1080))
	fmt.Println(math.Dim(5, 3), math.Dim(3, 5), math.Remainder(5, 3), math.Remainder(-7, 2), math.Remainder(7.5, 2))
	for _, x := range []float64{1e-10, 0.5, -0.5, 1, 3, -0.9999} {
		fmt.Println(math.Log1p(x), math.Expm1(x), math.Erf(x), math.Erfc(x))
	}
	for _, x := range []float64{5, 0.5, -1.5, 1e-5, 171, 172, 10.1} {
		fmt.Println(math.Gamma(x))
	}
	fmt.Println(math.Gamma(-2), math.Gamma(0), math.Erf(math.Inf(-1)), math.Erfc(30))
	fmt.Println(math.Nextafter(1, 2), math.Nextafter(1, 0), math.Nextafter(0, -1), math.Nextafter32(1, 2))
	for _, x := range []float64{2.5, 3.5, -2.5, 0.5, 1.5, -0.4, 1e16 + 1} {
		fmt.Print(math.RoundToEven(x), " ")
	}
	fmt.Println()
	b := math.Float64bits(1.5)
	fmt.Println(b, math.Float64frombits(b), math.Float64bits(-0.0), math.Float64bits(math.Inf(-1)))
	fmt.Println(math.Float32bits(1.5), math.Float32frombits(0x3fc00000), math.Float64frombits(0x7FF8000000000001))
	fmt.Printf("%x %T %T\n", math.Float64bits(math.Pi), math.Float64bits(1), math.Float32frombits(1))
}
