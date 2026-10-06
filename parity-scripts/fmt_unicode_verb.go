package main

// `%U`: at least four hex digits or the precision's count, `#` appending the
// quoted character when it is printable, and padding that ignores the `0` flag.

import "fmt"

func main() {
	for _, r := range []rune{'A', '⌘', 0x10FFFF, '\n', 0x1F600, 0x378, 0xAD, 0xFDD0} {
		fmt.Printf("%U|%#U|%.6U|%#8U|%-10U|%08U|%#.2U|\n", r, r, r, r, r, r, r)
	}
	fmt.Printf("%#U %U %U\n", -1, int64(1)<<40, uint8(200))
}
