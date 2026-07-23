package main

import "fmt"

func main() {
	sum := 0
	for i := 1; i <= 100; i++ {
		sum += i
	}
	fmt.Printf("sum 1..100 = %d\n", sum)
	greeting := "Go on " + "fusevm"
	fmt.Println(greeting)
}
