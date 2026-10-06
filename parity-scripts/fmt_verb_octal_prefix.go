package main

import "fmt"

func main() {
	fmt.Printf("%O|%#O|%8O|%-8O|%08O|%.4O|%#.4O|%O|%+O\n", 8, 8, 8, 8, 8, 8, 8, -9, 5)
	fmt.Printf("%O %O %O\n", []int{8, 9}, uint8(255), "s")
	fmt.Printf("%O %010O % O\n", 0, -64, 7)
}
