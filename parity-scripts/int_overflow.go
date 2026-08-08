package main

// Go's integers are fixed width and wrap on overflow (two's complement). go-rs
// used to route an overflowing `+` through the string-concatenation fallback,
// so int64max+1 printed as "92233720368547758071".
import "fmt"

func main() {
	var a int64 = 9223372036854775807
	fmt.Println(a+1, a+a, a*2, a*a)

	var c int = 9223372036854775807
	fmt.Println(c+1, c*2, c-(-1))

	var d int64 = -9223372036854775807
	fmt.Println(d-1, d*3)

	// Wrapping is consistent through a slice element, whose type go-rs only
	// learns at run time.
	xs := []int{9223372036854775807, -9223372036854775807}
	fmt.Println(xs[0]+1, xs[1]-1, xs[0]*2)

	// Conversions truncate to the target width.
	fmt.Println(int8(127), int16(-32768), int32(2147483647))
	fmt.Println(int8(200-100), uint8(255), int32(70000))

	// Bit operations and shifts.
	x := -8
	fmt.Println(x>>1, x<<2, uint(8)>>1, ^5, x&3, x|3, x^3)
	fmt.Println(1<<62, -(1 << 62))
}
