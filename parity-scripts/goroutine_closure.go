package main

import "fmt"

func main() {
	done := make(chan int)
	total := 60
	go func() {
		s := 0
		for i := 1; i <= 10; i++ {
			s += i
		}
		done <- s + total
	}()
	fmt.Println("result", <-done)
}
