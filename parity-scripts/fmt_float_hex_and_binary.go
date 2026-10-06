package main

// A float under `%x`, `%X` and `%b`, and `strconv.FormatFloat` with the `x`,
// `X` and `b` formats: Go's hexadecimal-mantissa and binary-exponent forms,
// with precision rounding the hex fraction, at the operand's own width.

import (
	"fmt"
	"math"
	"strconv"
)

func main() {
	for _, f := range []float64{3.5, 1, 0, -0.1, 1e300, 5e-324, math.MaxFloat64, 0.75, 1.0 / 3, 0x1.fffp-3} {
		fmt.Printf("%x %X %b %.3x %.0x %10.2x %-12x| %+x\n", f, f, f, f, f, f, f, f)
		fmt.Println(strconv.FormatFloat(f, 'x', -1, 64), strconv.FormatFloat(f, 'X', 4, 32),
			strconv.FormatFloat(f, 'b', -1, 64), strconv.FormatFloat(f, 'b', -1, 32),
			strconv.FormatFloat(f, 'x', 1, 64), strconv.FormatFloat(f, 'x', 20, 64))
	}
	var g float32 = 0.1
	fmt.Printf("%x %b %X\n", g, g, g)
	fmt.Printf("%x %x %b %X\n", math.Inf(1), math.NaN(), math.Inf(-1), -math.Inf(1))
	fmt.Println(strconv.FormatFloat(math.Inf(-1), 'x', -1, 64), strconv.FormatFloat(math.NaN(), 'b', -1, 64))
}
