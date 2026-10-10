// A shift count at or past the operand's width gives 0 for `<<` and for `>>`
// of a non-negative value, and the sign fill for `>>` of a negative one —
// whatever the count's own type, and through a compound assignment.
package main

import "fmt"

func main() {
	var s uint = 70
	var n int = 64
	var u8 uint8 = 0xF0
	fmt.Println(1<<s == 0, uint64(1)<<s, int64(-1)>>s, int8(-1)>>s)
	fmt.Println(1<<n, int32(5)<<n, uint32(1)<<31)
	fmt.Println(u8<<s, u8>>s, u8<<4, u8>>4, int8(-128)>>7, int8(-128)>>8)
	x := 12345
	x <<= s
	y := -12345
	y >>= s
	z := 99
	z <<= 3
	fmt.Println(x, y, z)
	var cnt uint64 = 1 << 40
	fmt.Println(1<<cnt, -1>>cnt, 7>>cnt)
	for i := uint(60); i < 68; i += 2 {
		fmt.Print(int64(1)<<i, " ", uint64(1)<<i, " ", int64(-8)>>i, "|")
	}
	fmt.Println()
	const big = 1 << 70
	fmt.Println(big>>65, big>>68, big/(1<<65), float64(big))
	fmt.Println(len("héllo"), len([3]int{}))
	const m = len("abc")
	var arr [m * 2]int
	fmt.Println(len(arr))
}
