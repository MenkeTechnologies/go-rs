package main

// How `%T` and `%#v` spell types that belong to no package: an anonymous
// struct is `struct { A int; B []main.pt }` (the empty one `struct {}`), `any`
// is `interface {}`, and a nil byte slice is `[]byte(nil)` as the operand but
// `[]uint8(nil)` nested inside another value.

import "fmt"

type pt struct{ X int }

type W struct {
	B  []byte
	R  []rune
	P  []pt
	M  map[string]pt
	MB map[byte][]rune
}

func main() {
	a := struct{}{}
	b := struct {
		A int
		B []pt
		C struct{ D string }
	}{A: 1}
	fmt.Printf("%T|%T\n", a, b)
	fmt.Printf("%v|%+v|%#v\n", a, b, b)
	fmt.Printf("%#v\n", a)
	xs := []struct{ N int }{{1}, {2}}
	fmt.Printf("%T %v %#v\n", xs, xs, xs)
	m := map[string]struct{}{"k": {}}
	fmt.Printf("%T %v\n", m, m)
	fmt.Printf("%T %T\n", []any{1}, [][]interface{}{})
	fmt.Printf("%#v\n", []any{1, "x"})

	var nb []byte
	var nr []rune
	var np []pt
	var nm map[pt]int
	fmt.Printf("%#v %#v %#v %#v\n", nb, nr, np, nm)
	fmt.Printf("%#v\n", W{})
	fmt.Printf("%#v\n", [][]pt{nil})
	bs := []byte("hi")
	fmt.Printf("%#v %#v\n", bs, [][]byte{bs, nil})
}
