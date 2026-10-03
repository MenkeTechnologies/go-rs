package main

import (
	"fmt"
	"math/bits"
)

func main() {
	fmt.Println(bits.Len(0), bits.Len(1), bits.Len(255), bits.Len64(1<<40), bits.Len8(7), bits.Len16(300), bits.Len32(1<<31))
	fmt.Println(bits.OnesCount(255), bits.OnesCount8(0xF0), bits.OnesCount16(0xFFFF), bits.OnesCount32(0x0F0F0F0F), bits.OnesCount64(1<<63|1))
	fmt.Println(bits.LeadingZeros64(1), bits.LeadingZeros32(1), bits.LeadingZeros8(1), bits.LeadingZeros16(256), bits.LeadingZeros(8))
	fmt.Println(bits.TrailingZeros(8), bits.TrailingZeros8(0), bits.TrailingZeros16(1024), bits.TrailingZeros32(96), bits.TrailingZeros64(1<<50))
	fmt.Println(bits.RotateLeft8(0x81, 1), bits.RotateLeft16(1, -1), bits.RotateLeft32(0x80000001, 4), bits.RotateLeft64(3, 62))
	fmt.Println(bits.Reverse32(1), bits.Reverse64(1), bits.ReverseBytes16(0x1234), bits.ReverseBytes32(0x12345678), bits.ReverseBytes64(0x0102030405060708))
	fmt.Println(bits.UintSize)

	// The (hi, lo) / (sum, carry) / (quo, rem) pairs are uint64: they keep
	// their type through the destructuring assignment and print unsigned.
	hi, lo := bits.Mul64(1<<40, 1<<40)
	fmt.Println(hi, lo)
	hi, lo = bits.Mul64(0xFFFFFFFFFFFFFFFF, 0xFFFFFFFFFFFFFFFF)
	fmt.Println(hi, lo)
	s, c := bits.Add64(1<<63, 1<<63, 1)
	fmt.Println(s, c)
	d, b := bits.Sub64(0, 1, 0)
	fmt.Println(d, b)
	h32, l32 := bits.Mul32(0xFFFFFFFF, 0xFFFFFFFF)
	fmt.Println(h32, l32)
	s32, c32 := bits.Add32(0xFFFFFFFF, 1, 0)
	fmt.Println(s32, c32)

	// Div64 runs Knuth's long division over uint64 intermediates declared
	// with `:=`; every one has to divide and compare unsigned.
	q, r := bits.Div64(1, 0, 7)
	fmt.Println(q, r)
	q, r = bits.Div64(0, 100, 7)
	fmt.Println(q, r)
	q, r = bits.Div64(5, 12345, 1<<63+5)
	fmt.Println(q, r)
	fmt.Println(bits.Rem64(1, 0, 7), bits.Rem64(9, 9, 7))
	var x uint8 = 200
	fmt.Println(bits.Len8(x), bits.OnesCount8(x))
}
