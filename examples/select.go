package main

import "fmt"

func main() {
	ch1 := make(chan int, 1)
	ch2 := make(chan int, 1)
	ch2 <- 7

	// select picks the ready channel (ch2)
	select {
	case v := <-ch1:
		fmt.Println("ch1", v)
	case v := <-ch2:
		fmt.Println("ch2", v)
	}

	// select with default when nothing is ready
	select {
	case v := <-ch1:
		fmt.Println("got", v)
	default:
		fmt.Println("no message")
	}

	// blocking select woken by a goroutine
	done := make(chan int)
	go func() {
		done <- 99
	}()
	select {
	case v := <-done:
		fmt.Println("done", v)
	}
}
