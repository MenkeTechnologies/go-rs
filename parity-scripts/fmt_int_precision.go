package main

// A precision on an *integer* verb is a minimum digit count, not the truncation
// it means for a string: `%.3d` of -7 is -007. The zeros go inside the sign and
// inside a `#` base prefix, a longer value is never shortened, and precision 0
// prints a zero value as the empty string.

import "fmt"

func main() {
	fmt.Printf("[%.0d][%.0d][%.d]\n", 0, 5, 0)
	fmt.Printf("[%.3d][%.3d][%+.3d]\n", -7, 12345, 7)
	fmt.Printf("[%05.2d][%-6.3d][% .3d]\n", 3, 4, 4)
	fmt.Printf("[%.3o][%.4b][%.4x][%.4X]\n", 8, 5, 255, 255)
	fmt.Printf("[%#.4x][%#.4o][%#.6b]\n", 255, 8, 5)

	// Precision does not reach the verbs that are not digit strings.
	fmt.Printf("[%.3c][%.3U][%.3q][%.3t]\n", 65, 65, 65, true)

	// A float's precision counts fraction digits, and keeps the `0` flag.
	fmt.Printf("[%.3f][%.0f][%05.1f][%.2e]\n", 1.5, 2.5, 3.5, 1234.5)

	// `%v` of an integer is `%d`, precision and zero-fill included.
	fmt.Printf("[%.2v][%.0v][%05v][%5v][%-5v|]\n", -3, 0, 42, 42, 42)

	// The rule applies element-wise inside a composite.
	fmt.Printf("[%.3d][%.3d]\n", []int{1, 22}, [2]int{7, 888})

	// A sized and an unsigned operand take it the same way.
	fmt.Printf("[%.2d][%.2d][%.4x]\n", uint8(7), int8(-7), uint64(255))
}
