package main

// `fmt`'s default float rendering is strconv's shortest 'g': the %e form when
// the decimal exponent is < -4 or >= 6, the plain decimal form otherwise. go-rs
// used to print the plain decimal form always, so 1e6 came out as "1000000".
import "fmt"

func main() {
	vals := []float64{
		0, 1, 1.5, 2, 100000, 999999, 1000000, 100000.5,
		0.0001, 0.00001, 0.1, 1.0 / 3.0, 3.14159265358979,
		1e15, 1e20, 1e21, 1e100, 123456789, 1234567, -2.5, -1e7,
	}
	for _, v := range vals {
		fmt.Println(v)
	}
	for _, v := range vals {
		fmt.Printf("%v|%f|%e|%.3f|%.2e\n", v, v, v, v, v)
	}
	for _, v := range vals {
		fmt.Printf("%g|%.3g|%.10g|%G|%.1g\n", v, v, v, v, v)
	}
	// Width, precision and sign flags.
	fmt.Printf("%8.2f|%-8.2f|%+.2f|%08.3f|%+d|%+.2e\n", 3.14159, 3.14159, 3.14159, 3.14159, 42, 3.14159)
	// Untyped constants have no signed zero, so -0.0 is exactly 0.
	fmt.Println(-0.0, 0.0)
	// Non-finite values.
	inf := 1.0
	zero := 0.0
	fmt.Println(inf/zero, -inf/zero)
	fmt.Printf("%v|%f|%e|%g\n", inf/zero, inf/zero, inf/zero, inf/zero)
	// A float-typed declaration converts its integer constant.
	var big float64 = 1000000
	fmt.Println(big, big/2)
}
