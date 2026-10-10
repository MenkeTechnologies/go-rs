// The shortest round-trip decimal of a float64 whose exact value sits halfway
// between two shortest candidates takes the even digit, in every verb that
// prints the shortest form.
package main

import (
	"fmt"
	"strconv"
)

func main() {
	x := 26481.0 / 262144.0 // 0.101016998291015625 exactly
	fmt.Println(x, -x)
	fmt.Println(strconv.FormatFloat(x, 'g', -1, 64), strconv.FormatFloat(x, 'e', -1, 64), strconv.FormatFloat(x, 'f', -1, 64))
	fmt.Printf("%v %g %.16g %.17g %.15f\n", x, x, x, x, x)
	for _, v := range []float64{0.3, 2.675, 1e23, 8.41e21, 5e-324, 123456789012345678, 4025693.25} {
		fmt.Println(v, float32(v))
	}
}
