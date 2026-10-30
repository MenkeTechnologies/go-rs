package main

import "fmt"

type R struct{ C rune }

// An untyped rune constant defaults to `rune` (int32): a variable it
// initializes is an int32 — it wraps at 32 bits and `%T` names it — unless the
// other operand of its expression is typed, whose type it takes.
func main() {
	r := 'a'
	fmt.Printf("%T %v %c\n", r, r, r)
	var x = 'b' + 1
	fmt.Printf("%T %v\n", x, x)
	n := 5
	y := 'c' + n
	z := n + 'c'
	fmt.Printf("%T %T %v %v\n", y, z, y, z)
	fmt.Printf("%T %T %v\n", 'x', 'x'-'a', 'x'-'a')
	d := 'z' - 'a'
	fmt.Printf("%T %v %s %s\n", d, d, string(r), string(r+1))
	var b byte = 'q'
	c := b - 'a'
	fmt.Printf("%T %v\n", c, c)
	m := map[rune]int{'a': 1}
	m['b']++
	fmt.Println(m, R{'z'})
	f := 1.5 * 'a'
	fmt.Printf("%T %v\n", f, f)
	s := "abc"
	k := s[0] - 'a'
	fmt.Printf("%T %v\n", k, k)
	big := 'a' << 20
	fmt.Printf("%T %v\n", big, big)
	var i64 int64 = 'a'
	fmt.Printf("%T\n", i64)
	r2 := r
	r2 += 0x7fffffff
	fmt.Println(r2)
	upper := 'a' - 32
	fmt.Printf("%T %c\n", upper, upper)
}
