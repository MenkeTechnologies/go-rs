package main

import "fmt"

const half = .5

func main() {
	x := .25
	y := .5e1
	var z float32 = .1
	fmt.Println(half, x, y, z, .5+.5, 1+.5, -.75)
	fmt.Printf("%T %T %v\n", .5, x, [2]float64{.5, 1.})
	s := []int{1, 2, 3}
	fmt.Println(s[1:], len(s[:2]))
}
