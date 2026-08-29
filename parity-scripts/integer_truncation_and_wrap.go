package main

import "fmt"

func main() {
	fmt.Println(7/2, -7/2, 7/-2, -7/-2)
	fmt.Println(7%3, -7%3, 7%-3, -7%-3)
	var i8 int8 = 127
	i8++
	var u8 uint8 = 0
	u8--
	fmt.Println(i8, u8)
	var u32 uint32 = 1 << 31
	fmt.Println(u32*2, u32<<1)
	var neg int32 = -8
	fmt.Println(neg>>1, uint32(4294967288)>>1)
	fmt.Println(1<<10, 1>>1)
	f := 2.9
	fmt.Println(int(f), int(-f))
	fmt.Println(len("héllo"), len([]rune("héllo")))
	for i, r := range "hé" {
		fmt.Println(i, r)
	}
}
