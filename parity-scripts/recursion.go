package main

import "fmt"

func fib(n int) int {
	if n < 2 {
		return n
	}
	return fib(n-1) + fib(n-2)
}

func fact(n int) int {
	if n <= 1 {
		return 1
	}
	return n * fact(n-1)
}

func main() {
	for i := 0; i <= 15; i++ {
		fmt.Printf("fib(%d)=%d fact=%d\n", i, fib(i), fact(i))
	}
}
