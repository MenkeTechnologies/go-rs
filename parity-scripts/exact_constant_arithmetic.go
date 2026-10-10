// Untyped constant arithmetic is exact: `iota` blocks with skips, shifts past 64
// bits, a named constant above int64 converted to float, rational division and
// `len` of a string constant sizing an array.
package main

import "fmt"

const (
	A = 1 << iota
	B
	C
	D = iota * 10
	E
	_
	G
)

const big = 1 << 100
const f = big >> 98
const h = 5 / 2
const hf = 5 / 2.0
const r = 'a' + 1
const s = "x" + "y"
const typed int8 = 100
const fl = 1e3
const mix = 'a' * 2.0
const cm = 7 % 3
const neg = -7 / 2
const negm = -7 % 3
const bits = 0xFF &^ 0x0F
const xo = 6 ^ 3
const nt = ^0
const ut = ^uint8(0)
const shf = 1.0 << 3
const (
	KB float64 = 1 << (10 * (iota + 1))
	MB
	GB
)

type Color int

const (
	Red Color = iota
	Green
	Blue
)

func (c Color) String() string { return [...]string{"R", "G", "B"}[c] }

const (
	x0, y0 = iota, iota * 2
	x1, y1
	x2, y2
)

func main() {
	fmt.Println(A, B, C, D, E, G)
	fmt.Println(f, h, hf, r, s, typed, fl, mix, cm, neg, negm, bits, xo, nt, ut, shf)
	fmt.Println(KB, MB, GB)
	fmt.Println(Red, Green, Blue)
	fmt.Printf("%v %d %T %T %T\n", Blue, Blue, fl, hf, mix)
	fmt.Println(x0, y0, x1, y1, x2, y2)
	var u8 uint8 = 255
	fmt.Println(u8 + 1 - 1)
	const c1 = 10
	var fv float64 = c1 / 4
	var fw float64 = c1 / 4.0
	fmt.Println(fv, fw)
	fmt.Println(big/(1<<98), float64(big))
	var d = 1 << 3.0
	fmt.Printf("%T %v\n", d, d)
	fmt.Println(len("héllo"), len([3]int{}), len(s))
	const m = len("abc")
	var arr [m * 2]int
	fmt.Println(len(arr))
	fmt.Println(7.0/2, 7/2, 7/2.0, 1/3.0)
	fmt.Println(0.1+0.2, float32(0.1)+float32(0.2))
	const huge = 1e300 * 1e300 / 1e300
	fmt.Println('a', 'a'+1, string(rune('a'+1)))
}
