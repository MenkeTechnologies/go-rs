// `x := T(v)` of a basic type declares a `T`: it wraps at its width and
// `%T` names it. `n := 0` declares an `int` and `f := 0.5` a `float64`, so
// `&n` handed to `fmt.Sscan` or to a type switch is an `*int`.
package main

import "fmt"

func show(v any) string {
	switch v.(type) {
	case *int:
		return "*int"
	case *float64:
		return "*float64"
	case *int8:
		return "*int8"
	case *uint8:
		return "*uint8"
	case *int32:
		return "*int32"
	}
	return "other"
}

func parse(s string) int {
	n := 0
	fmt.Sscanf(s, "%d", &n)
	return n
}

func main() {
	g := int8(127)
	g++
	u := uint8(250)
	u += 10
	f := float32(0.1)
	w := uint32(1) << 31
	w <<= 1
	b := byte('a')
	r := rune(66)
	fmt.Println(g, u, f, w, b, r)
	fmt.Printf("%T %T %T %T %T %T\n", g, u, f, w, b, r)

	a := 0
	c := 0.5
	d := len("ab")
	e := 1 + 2
	k := -3
	j := 1 << 3
	q := 'x' + 1
	m := 2 * 1.5
	var v uint8 = 2
	h := v + 1
	for _, p := range []any{&a, &c, &d, &e, &k, &j, &q, &m, &g, &h} {
		fmt.Print(show(p), " ")
	}
	fmt.Println()
	fmt.Printf("%T %T %T %T\n", &a, &c, &q, &h)

	fmt.Println(parse("42"), parse("-7"))
	x := 0
	fmt.Sscan("5", &x)
	y := 0.0
	fmt.Sscan("2.5", &y)
	fmt.Println(x, y)
}
