// Fixed-width integer arithmetic wraps at each type's own width — add, sub,
// mul, negate, shifts, conversions between widths and signedness, and
// the MinInt / -1 corner.
package main

import (
	"fmt"
	"math"
)

func main() {
	var a int8 = 127
	a++
	fmt.Println(a)
	var b uint8 = 0
	b--
	fmt.Println(b)
	var c int16 = -32768
	c--
	fmt.Println(c, -c)
	var d uint16 = 65535
	d += 2
	fmt.Println(d)
	var e int32 = math.MaxInt32
	e *= 2
	fmt.Println(e)
	var f uint32 = 1 << 31
	f <<= 1
	fmt.Println(f)
	var g int64 = math.MaxInt64
	g++
	fmt.Println(g)
	var h uint64 = math.MaxUint64
	h++
	fmt.Println(h)
	h = 3
	h -= 5
	fmt.Println(h)
	var i int = math.MinInt64
	fmt.Println(i/-1 == i, i%-1)
	var j int8 = -128
	fmt.Println(j/-1, j%-1, -j)
	var k uint8 = 200
	fmt.Println(k+100, k*2, k<<1, k>>1, ^k)
	var l int8 = -5
	fmt.Println(l>>1, l<<5, uint8(l), int(l), uint32(l), uint64(l))
	var m uint32 = 0xdeadbeef
	fmt.Println(int32(m), int16(m), int8(m), uint16(m))
	var s uint = 70
	fmt.Println(1<<s == 0, uint64(1)<<s, int64(-1)>>s, int8(-1)>>s)
	var n int64 = -9
	fmt.Println(n/2, n%2, n>>1, uint64(n)>>60)
	fmt.Println(math.MaxInt8, math.MinInt8, math.MaxUint32)
	var x uint8 = 255
	var y uint8 = 2
	fmt.Println(x*y, x+y, y-x, x/y, x%y)
	var u uint = 0
	u--
	fmt.Println(u, u>>63)
	var sh8 uint8 = 1
	fmt.Println(sh8<<7, sh8<<8, sh8<<9)
	fmt.Println(int32(math.MaxInt32)+int32(sh8) < 0)
	var r rune = 'a'
	r += 0x10FFFF
	fmt.Println(r)
	var bb byte = 'z'
	bb += 10
	fmt.Println(bb, string(rune(bb)))
	f64 := 3.99
	fmt.Println(int(f64), int(-f64), uint8(int(f64)))
	var i32 int32 = -1
	fmt.Println(uint16(i32), uint32(i32)>>31, int64(uint32(i32)))
	fmt.Printf("%d %x %o %b %X\n", int8(-128), int8(-1), uint8(255), int8(-3), uint16(65535))
	fmt.Printf("%08b %#x %#o %+d % d\n", uint8(5), 255, 8, 5, 5)
}
