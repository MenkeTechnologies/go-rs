package main

import (
	"fmt"
	"math"
)

func main() {
	nan, inf := math.NaN(), math.Inf(1)
	z := 0.0
	nz := math.Copysign(0, -1)
	fmt.Println(math.Max(nan, 1), math.Max(1, nan), math.Min(nan, 1), math.Min(1, nan))
	fmt.Println(math.Max(nan, inf), math.Max(-inf, nan), math.Min(nan, -inf), math.Min(inf, nan))
	fmt.Println(math.Max(0, nz), math.Max(nz, 0), math.Min(0, nz), math.Min(nz, 0))
	fmt.Println(math.Signbit(math.Max(nz, nz)), math.Signbit(math.Min(0, nz)), math.Max(-3, -2), math.Min(-3, -2), math.Max(inf, 1), math.Min(-inf, 1))
	fmt.Println(math.Signbit(min(0.0+z, nz)), math.Signbit(min(nz, z)), math.Signbit(max(nz, z)), math.Signbit(max(z, nz)), math.Signbit(max(nz, nz)))
	fmt.Println(max(nan, 1), min(1, nan), max(2.5, 1), min(2, 3.5), max("a", "b"), min(3, 1, 2))
}
