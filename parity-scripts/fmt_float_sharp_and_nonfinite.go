package main

import (
	"fmt"
	"math"
)

// The `#` flag on a float verb keeps the decimal point and, for %g/%G/%x,
// restores trailing zeros up to the precision (fmt.fmtFloat's f.sharp branch).
// An infinity or NaN is never zero-padded, a positive infinity keeps its `+`
// unless the space flag replaces it, and a float32 pads and rejects verbs as
// the float it is.
func main() {
	var f32 float32 = 0.1
	fmt.Printf("%#g|%#g|%#.3g|%#G|%#e|%#.0e|%#.0f|%#f\n", 1.0, 123456789.0, 2.0, 1e-7, 1.0, 5.0, 3.0, 2.5)
	fmt.Printf("%#x|%#X|%#.2x|%#x|%#x|%#.3x\n", 3.0, 1.0, 1.0, 0.0, -2.0, 1.5)
	fmt.Printf("%#g|%#g|%#g|%#.0g|%#.1g|%#g\n", 0.0, 100000.0, 1e21, 7.0, 123.0, f32)
	fmt.Printf("%#10.3g|%#-10g|%#+g|%#08.2f|% #g|%+#e\n", 1.0, 2.0, 3.0, -3.0, 4.0, 0.0)
	fmt.Printf("%#x|%#X|%#b\n", []float64{1, 0.5}, float32(2), 1.0)
	fmt.Printf("%#v|%#v|%#v\n", 1.0, []float64{1, 2.5}, map[string]float64{"a": 1})

	i, n, m := math.Inf(1), math.NaN(), math.Inf(-1)
	for _, f := range []string{"%g", "%e", "%08g", "%08.2e", "%-08g|", "%+08f", "% 08g", "% g", "%+ e", "%#08x", "%08v"} {
		fmt.Printf(f+"|"+f+"|"+f+"\n", i, n, m)
	}
	inf32 := float32(math.Inf(1))
	fmt.Printf("%08v|% g|%+v\n", inf32, inf32, []float64{i, n})

	var a float32 = -1.5
	fmt.Printf("%08v|%-8v|%+08v|%08d|%q|%s\n", a, a, a, a, a, a)
}
