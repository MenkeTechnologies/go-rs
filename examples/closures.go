package main

import "fmt"

func main() {
	// closure capturing a local
	factor := 3
	triple := func(x int) int {
		return x * factor
	}
	fmt.Println(triple(5))

	// IIFE
	fmt.Println(func(a int, b int) int { return a + b }(10, 20))

	// closure in a goroutine capturing a channel + value
	done := make(chan int)
	msg := 42
	go func() {
		done <- msg
	}()
	fmt.Println(<-done)

	// counter-style closure (capture is by value — documented)
	base := 100
	addBase := func(n int) int { return base + n }
	fmt.Println(addBase(1), addBase(2))
}
