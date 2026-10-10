// strconv.ParseFloat accepts Go's whole float-literal grammar: hex floats
// (the `p` exponent is mandatory), `_` digit separators, the case-insensitive
// specials (unsigned NaN only), and rounds to float32 when bitSize is 32.
package main

import (
	"fmt"
	"strconv"
)

func main() {
	for _, s := range []string{
		"1", "-1.5e3", "+.5", "5.", ".", "", "e5", "1e", "1e+",
		"0x1p-2", "0x1.8p1", "0X1P+3", "0x10", "0x.8p0", "0x1p", "0x_1p0",
		"1_0.5", "1__0", "_1", "1_", "1e+0_1",
		"inf", "-Inf", "+INFINITY", "infinit", "nan", "NaN", "+nan", "-nan",
		"1e308", "1e309", "-1e309", "4.9e-324", "1e-400", "3.4e39",
		"0.1", " 1", "1 ", "0b101", "1p3", "-0",
	} {
		f, err := strconv.ParseFloat(s, 64)
		g, err32 := strconv.ParseFloat(s, 32)
		fmt.Println(strconv.Quote(s), f, err, g, err32)
	}
}
