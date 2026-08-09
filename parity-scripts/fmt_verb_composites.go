// Every `fmt` verb except `%v` and `%T` applies element-wise to a composite
// operand, with the flags, width and precision belonging to each element rather
// than to the whole rendering. The one operand that is not a list is a `[]byte`
// under `%s`/`%q`/`%x`/`%X`, which is the text it holds — at every depth. The
// `[]byte` and `[]int` lines sit next to each other on purpose: they hold the
// same numbers, so a formatter that decides byte-ness from the values alone
// cannot get both right.
package main

import "fmt"

type pt struct{ x, y int }

func main() {
	ws := []string{"alpha", "beta"}
	is := []int{97, 98}
	bs := []byte("abc")
	ms := map[string]string{"b": "y", "a": "x"}
	mi := map[string]int{"k": 3}
	ar := [2]string{"p", "q"}
	fs := []float64{1.5, 2.5}
	nest := [][]string{{"a"}, {"b", "c"}}
	nb := [2][]byte{[]byte("ab"), []byte("c")}
	var nilS []string
	var nilB []byte
	p := pt{-9, 20}

	// %q distributes, and quotes an integer element as a rune literal.
	fmt.Printf("%q|%q|%q|%q\n", ws, is, bs, ms)
	fmt.Printf("%q|%q|%q|%q\n", ar, fs, nest, nb)
	fmt.Printf("%q|%q|%q\n", nilS, nilB, p)

	// The numeric and text verbs distribute the same way, and report the
	// operands they do not accept as Go's bad-verb form.
	fmt.Printf("%d|%d|%d|%d\n", is, mi, nest, p)
	fmt.Printf("%s|%s|%s|%s\n", ws, is, bs, ms)
	fmt.Printf("%x|%X|%x|%x\n", bs, is, ms, nb)
	fmt.Printf("%f|%e|%g\n", fs, fs, fs)
	fmt.Printf("%o|%b|%c|%U\n", is, is, is, is)
	fmt.Printf("%t|%t\n", []bool{true, false}, is)

	// Width, precision and the flags apply per element.
	fmt.Printf("%8q|%-6q|%#q|%5d|%05d\n", ws, ws, ws, is, is)
	fmt.Printf("%.2q|%.1s|%6.2f\n", ws, ws, fs)

	// A negative operand under a base verb is a sign and a magnitude, not the
	// two's-complement bit pattern; `#` writes the base prefix after the sign.
	n := -9
	fmt.Printf("%x|%X|%o|%b\n", n, n, n, n)
	fmt.Printf("%#x|%#X|%#o|%#b\n", n, n, n, n)
	fmt.Printf("%#08x|%08x|%-8x|%+x|%+o\n", n, n, n, n, n)

	// A rune outside Unicode is the replacement character, and `%U` reads the
	// operand's bits unsigned.
	runes := []int{0, 7, 0x1f, 0x20, 0x41, 0x7f, 0x80, 0x9f, 0xa0, 0xa9, 0x4e16, 0xfffd, 0xe000, 1114112, -9}
	fmt.Printf("%q\n", runes)
	fmt.Printf("%c\n", runes)
	fmt.Printf("%U\n", runes)

	// The written element type is what separates these two `%T`s, and what
	// makes `%#v` of a byte slice Go source for one.
	fmt.Printf("%T|%T|%T|%T\n", bs, is, nilB, nb)
	fmt.Printf("%v|%+v|%#v|%#v\n", bs, p, bs, ms)
	fmt.Printf("%q|%s|%x\n", nilB, nilS, nilB)

	// A verb with no composite in sight still reports a bad operand.
	fmt.Printf("%d|%q|%t|%s\n", "k", true, 3, false)
	fmt.Printf("%5d|%05d|%.2d\n", "k", "k", "kkkk")
}
