// Constants exactly at an integer type's limits convert and assign without
// error; an untyped constant may exceed `int` while it stays a constant; a
// float divided by a constant zero is ±Inf at run time.
package main

import "fmt"

const big = 1 << 70

const (
	kilo = 1000 * (iota + 1)
	mega
)

type Small int8

func main() {
	fmt.Println(int8(127), int8(-128), uint8(255), byte(0), int16(-32768), uint16(65535))
	fmt.Println(int32(1<<31-1), uint32(1<<32-1), int64(1<<63-1), rune(0x10FFFF))
	var s Small = -128
	var b byte = 255
	var u uint64 = 18446744073709551615
	var e uint64 = 1<<64 - 1
	fmt.Println(s, b, u, e)
	fmt.Println(big>>65, int64(big>>8), uint16(mega))
	x := 1<<62 + (1<<62 - 1)
	fmt.Println(x)
	f := 5.0
	fmt.Println(f/0, -f/0)
	n := 9
	fmt.Println(n/3, n%4)
}
