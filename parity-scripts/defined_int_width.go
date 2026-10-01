package main

import "fmt"

// A defined type over a sized or unsigned integer computes at its base's width
// and signedness: `type B uint8` wraps at 8 bits, and `type U uint64` divides,
// shifts and compares unsigned.

type U uint64
type B uint8
type I8 int8
type W uint16

type Reg struct {
	Flags B
	Acc   U
}

func (b B) Next() B { return b + 1 }

func (u U) Half() U { return u >> 1 }

// The xorshift generator from Go's sort package: compound assignment through
// a pointer to a defined uint64.
type xorshift uint64

func (r *xorshift) Next() uint64 {
	*r ^= *r << 13
	*r ^= *r >> 7
	*r ^= *r << 17
	return uint64(*r)
}

func main() {
	var x U = 1 << 63
	fmt.Println(x>>7, x/3, x%7, x > 5, x.Half())
	fmt.Println(x/2, x > 1, x%10, uint64(x)>>60, float64(x))
	fmt.Printf("%d %v %T\n", x, x, x)

	var b B = 250
	b += 10
	fmt.Println(b, b*2)
	b = 255
	fmt.Println(b.Next(), b+1, -b, ^b, b<<1)
	var i I8 = 127
	i++
	fmt.Println(i)

	n := 300
	fmt.Println(B(n), I8(n), W(n*1000))
	bs := []B{250, 251}
	bs[0] += 10
	fmt.Println(bs)
	r := Reg{Flags: 200}
	r.Flags += 100
	r.Acc--
	fmt.Println(r.Flags, r.Acc, r.Acc > 1)
	var w W = 30000
	w += 40000
	fmt.Println(w)
	m := map[string]I8{"a": 100}
	m["a"] += 100
	fmt.Println(m["a"])
	for i := I8(120); i > 0 && i < 127; i += 3 {
		fmt.Print(i, " ")
	}
	fmt.Println()

	rng := xorshift(12345)
	for k := 0; k < 5; k++ {
		fmt.Println(rng.Next(), uint(rng.Next())&63)
	}
	top := xorshift(1 << 63)
	fmt.Println(top.Next())
}
