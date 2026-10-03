package main

// A comment saying trust, beside a block comment that is not ASCII, is
// ordinary Go with no inline Rust in it.

import "fmt"

/* café — naïve, 1µs */
func main() {
	trusted := "rust"
	/* señor */
	fmt.Println(trusted, len(trusted))
}
