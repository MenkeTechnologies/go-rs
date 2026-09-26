// An array length written as a constant expression rather than a literal —
// `[N]T`, `[N*2]T`, `[int(N)]T`, a constant declared after its use, a local
// constant, and a defined type over such an array (`type d [MaxCase]rune`, the
// shape `unicode` declares its case tables with). Each must stay a fixed-size
// *value* array: the zero value is N elements, a short literal is padded, and
// a copy does not alias.
package main

import "fmt"

type d [MaxCase]rune

type caseRange struct {
	Lo, Hi uint32
	Delta  d
}

type table []caseRange

// Declared before the constant it is sized by, and built from an elided
// literal of the defined array type.
var cases = table{
	caseRange{0x49, 0x49, d{0, 0x131 - 0x49, 0}},
	{0x69, 0x69, d{0x130 - 0x69}},
}

const MaxCase = 3
const (
	small = iota + 2
	big
)

// A one-name generic parameter list is still a type-parameter list.
type box[T any] [2]T

func main() {
	var zero [MaxCase * 2]int
	fmt.Println(len(zero), zero)

	short := [MaxCase]string{"a"}
	fmt.Printf("%d %q\n", len(short), short)

	conv := [int(big)]bool{}
	fmt.Println(len(conv), conv)

	const local = 4
	var grid [local][small]int
	grid[1][0] = 7
	fmt.Println(len(grid), len(grid[0]), grid)

	a := d{1, 2}
	b := a
	b[0] = 9
	fmt.Println(a, b, len(a))

	for _, c := range cases {
		fmt.Println(c.Lo, c.Hi, c.Delta)
	}
	fmt.Printf("%T %T\n", zero, a)

	bx := box[int]{1, 2}
	fmt.Println(bx, len(bx))
}
