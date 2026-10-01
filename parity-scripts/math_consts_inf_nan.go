// The math package's sized-integer limits and float constants, and the IEEE
// special values: Inf/NaN build them, IsInf/IsNaN/Signbit test them, and
// Copysign moves a sign — including onto a NaN or a zero.
package main

import (
	"fmt"
	"math"
)

func main() {
	fmt.Println(math.MaxInt8, math.MinInt8, math.MaxInt16, math.MinInt16)
	fmt.Println(math.MaxInt32, math.MinInt32, math.MaxUint8, math.MaxUint16, math.MaxUint32)
	var i32 int32 = math.MaxInt32
	i32++
	var u8 uint8 = math.MaxUint8
	u8++
	fmt.Println(i32, u8)
	fmt.Println(math.MaxFloat64, math.SmallestNonzeroFloat64)
	fmt.Println(math.MaxFloat32, math.SmallestNonzeroFloat32)
	fmt.Println(math.Phi, math.SqrtE, math.SqrtPi, math.SqrtPhi)
	fmt.Println(math.Ln2, math.Log2E, math.Ln10, math.Log10E)
	pos, neg, nan := math.Inf(1), math.Inf(-1), math.NaN()
	fmt.Println(pos, neg, nan, math.Inf(0))
	fmt.Println(math.IsInf(pos, 1), math.IsInf(pos, -1), math.IsInf(neg, 0), math.IsInf(1e308, 0))
	fmt.Println(math.IsNaN(nan), math.IsNaN(pos), nan == nan, nan != nan)
	var a, b any = nan, pos
	fmt.Println(a == a, b == b, a != b)
	fmt.Println(math.Signbit(0), math.Signbit(math.Copysign(0, -1)), math.Signbit(neg), math.Signbit(2))
	fmt.Println(math.Copysign(3, -1), math.Copysign(-2.5, 1), math.IsNaN(math.Copysign(nan, -1)))
	fmt.Printf("%.3f %v %8.2f|\n", math.Phi, math.Max(pos, 1), neg)
	lo := math.Inf(1)
	for _, v := range []float64{3, -2, 7} {
		lo = math.Min(lo, v)
	}
	fmt.Println(lo)
}
