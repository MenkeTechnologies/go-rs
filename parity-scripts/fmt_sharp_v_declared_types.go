package main

import (
	"errors"
	"fmt"
)

// %#v writes every unsigned integer in hex (printValue's fmt0x64(v, true)),
// and the declared type of a field, element, key or value decides what the
// value alone cannot: the name of a nil interface (`interface {}(nil)`), of a
// nil or unboxed defined composite (`main.Names(nil)`), and the element type of
// a slice built without one (`[]uint8{0x68, 0x69}`).

type Lookup map[string]int
type Names []string
type ID uint16
type F func()
type Rec struct {
	N  Names
	L  Lookup
	Ks []ID
	E  map[string]error
	B  []byte
	U  uint
	W  uint64
}

func main() {
	var x uint = 7
	var b byte = 'y'
	fmt.Printf("%#v %#v %#v %#v\n", x, b, []uint{1}, map[string]uint16{"a": 1})
	fmt.Printf("%#v\n", []any{nil, 1, "a"})
	var l Lookup
	fmt.Printf("%v %#v %T|%v %#v\n", Lookup(nil), Lookup(nil), Lookup(nil), l, l)
	var f F
	fmt.Printf("%#v\n", f)
	m := map[string]any{"z": nil, "a": []int{1}, "m": ID(2)}
	fmt.Printf("%v|%+v|%#v|%T\n", m, m, m, m)
	e := map[string]error{"x": nil, "y": errors.New("boom")}
	fmt.Printf("%v|%T\n", e, e)
	r := Rec{Ks: []ID{1, 255}, B: []byte("hi"), U: 10, W: 1 << 63}
	fmt.Printf("%#v\n", r)
	r.N = Names{"q"}
	r.L = Lookup{"k": 1}
	fmt.Printf("%#v\n%v\n", r, r)
	fmt.Printf("%#v %#v %#v\n", []ID{3}[0:1], [2]ID{4, 5}, map[ID]Names{7: {"z"}, 1: nil})
	var u8 uint8 = 200
	var up uintptr = 16
	fmt.Printf("%#v %#v %#v %#v\n", u8, up, uint32(1)<<31, ^uint64(0))
	fmt.Printf("%#v %#v\n", []error{nil}, map[int]any{1: nil})
}
