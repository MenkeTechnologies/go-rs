package main

import "fmt"

func gen(ch chan int, start, n int) {
	for i := 0; i < n; i++ {
		ch <- start + i
	}
}

func main() {
	a := make(chan int, 5)
	b := make(chan int, 5)
	go gen(a, 10, 5)
	go gen(b, 100, 5)
	sum := 0
	for got := 0; got < 10; got++ {
		select {
		case v := <-a:
			sum += v
		case v := <-b:
			sum += v
		}
	}
	fmt.Println("fan-in sum", sum)
}
