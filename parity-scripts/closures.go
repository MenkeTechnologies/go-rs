package main

import "fmt"

func main() {
	base := 100
	add := func(n int) int { return base + n }
	fmt.Println(add(1), add(2), add(3))

	fmt.Println(func(a, b int) int { return a * b }(6, 7))

	acc := 0
	square := func(x int) int { return x * x }
	for i := 1; i <= 5; i++ {
		acc += square(i)
	}
	fmt.Println("acc", acc)
}
