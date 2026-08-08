// Fixed-width integer arithmetic: every sized type wraps at its own width, not
// at 64 bits — through ++/--, compound assignment, binary operators, unary
// complement, struct fields, slice elements, and function results.
package main

import "fmt"

type box struct {
	b uint8
	s int16
	w int32
}

func addByte(a, b uint8) uint8 { return a + b }

// FNV-1a: the canonical shape that needs real uint32 wrapping.
func fnv32(s string) uint32 {
	var h uint32 = 2166136261
	for i := 0; i < len(s); i++ {
		h ^= uint32(s[i])
		h *= 16777619
	}
	return h
}

func main() {
	// ++ / -- at each width.
	var i8 int8 = 127
	i8++
	var u8 uint8 = 0
	u8--
	var i16 int16 = 32767
	i16++
	var u16 uint16 = 65535
	u16++
	var i32 int32 = 2147483647
	i32++
	var u32 uint32 = 4294967295
	u32++
	fmt.Println(i8, u8, i16, u16, i32, u32)

	// 64-bit types keep 64-bit wrapping.
	var n int = 127
	n++
	var i64 int64 = 9223372036854775807
	i64++
	fmt.Println(n, i64)

	// Binary operators take the width from whichever operand is sized.
	var a int8 = 100
	var b int8 = 100
	fmt.Println(a+b, a*3, a-(-100), b/3)
	var x uint8 = 200
	fmt.Println(x+100, x*2, x<<1, x>>1, x-201)

	// Compound assignment.
	var c uint16 = 65530
	c += 10
	var d int16 = -32760
	d -= 100
	fmt.Println(c, d)

	// Unary complement and negation.
	fmt.Println(^uint8(0), ^uint16(0), ^int8(0), ^int16(1))
	var neg int8 = -128
	fmt.Println(-neg)

	// Struct fields.
	bx := box{b: 250, s: 32760, w: 2147483640}
	bx.b += 10
	bx.s += 10
	bx.w += 10
	fmt.Println(bx.b, bx.s, bx.w, bx)

	// Slice elements, both from a literal and from make.
	xs := []uint8{250, 3}
	xs[0] += 10
	xs[1] *= 100
	ys := make([]int16, 2)
	ys[0] = 32760
	ys[0] += 20
	fmt.Println(xs, ys)

	// Function parameters and results.
	fmt.Println(addByte(200, 100), addByte(1, 2))
	fmt.Println(fnv32("hello"), fnv32("go-rs"), fnv32(""), fnv32("the quick brown fox"))

	// byte and rune are uint8 and int32.
	var by byte = 255
	by++
	var r rune = 'A'
	r += 2
	fmt.Println(by, r, string(r))

	// Conversions still truncate, and a narrowed value round-trips. (The
	// operands are variables: Go rejects an out-of-range *constant* conversion
	// at compile time, which go-rs does not yet diagnose — see BUGS.md.)
	var wide int = 300
	var wider int = 5000000000
	var widest int = 70000
	fmt.Println(int8(wide), int32(wider), uint8(wide+211), int16(widest))
	fmt.Println(uint8(wide), int8(wide)+1, int32(wider)/2)

	// A loop that only terminates if the counter wraps.
	var k uint8 = 0
	steps := 0
	for {
		k += 7
		steps++
		if k == 0 {
			break
		}
	}
	fmt.Println("wrapped after", steps)
}
