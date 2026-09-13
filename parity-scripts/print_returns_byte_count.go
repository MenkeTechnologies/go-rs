package main

import "fmt"

// `fmt.Print`, `Printf` and `Println` return `(n int, err error)`, where `n` is
// the number of BYTES written — not runes, and not the operand count. Every
// print here is in a value position, so each one has to build that pair; the
// statement form below must keep answering nothing, which is what the rest of
// this corpus already covers.

func main() {
	n, err := fmt.Println("x")
	fmt.Println(n, err)

	// Bytes, not runes: "héllo\n" is 7 bytes because é is two of them.
	n, _ = fmt.Println("héllo")
	fmt.Println(n)

	// The separators and the trailing newline count too.
	n, _ = fmt.Println(1, 2, 3)
	fmt.Println(n)

	// `Print` adds spaces only between operands that are both non-strings.
	n, _ = fmt.Print("xy")
	fmt.Print("\n", n, "\n")

	n, _ = fmt.Print(1, 2)
	fmt.Print("\n", n, "\n")

	// `Printf` counts the rendered result, not the format string.
	n, _ = fmt.Printf("%s", "abc")
	fmt.Print("\n", n, "\n")

	n, _ = fmt.Printf("%d-%s\n", 5, "z")
	fmt.Print(n, "\n")

	// A count accumulated across a loop: each iteration writes "i\n", which is
	// two bytes for a single digit.
	total := 0
	for i := 0; i < 4; i++ {
		w, _ := fmt.Println(i)
		total += w
	}
	fmt.Println("total", total)

	// A print nested as an argument is itself in a value position, so its pair
	// is built even though the enclosing print is a statement.
	fmt.Println(fmt.Sprintf("%d", 7))
}
