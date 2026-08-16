package main

// Signed and unsigned overflow wraps at each type's own width. `int`/`int64`
// wrap where the underlying `i64` already does; every narrower type has to be
// wrapped explicitly, and reading the sign bit at the wrong width flips it.

import "fmt"

func main() {
	var m int64 = 9223372036854775807
	fmt.Println(m+1, m*2, -m-2)
	var i32 int32 = 2147483647
	fmt.Println(i32+1, i32*2)
	var i16 int16 = 32767
	fmt.Println(i16+1, i16*3)
	var i8 int8 = 127
	fmt.Println(i8+1, i8*2, i8*3)
	var u8 uint8 = 200
	fmt.Println(u8+100, u8*2)
	var u16 uint16 = 65535
	fmt.Println(u16+1, u16*2)
	var u32 uint32 = 4294967295
	fmt.Println(u32+1, u32/2)
	n := 1
	for k := 0; k < 64; k++ {
		n *= 2
	}
	fmt.Println(n)
	var s8 int8 = -128
	fmt.Println(s8-1, -s8)
	fmt.Println(m<<1, m>>1)
}
