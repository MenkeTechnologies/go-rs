// len/cap of a channel read its buffer, and the directional channel types
// <-chan T / chan<- T are accepted in signatures and declarations.
package main

import "fmt"

func gen(n int) <-chan int {
	ch := make(chan int, n)
	for i := 0; i < n; i++ {
		ch <- i
	}
	close(ch)
	return ch
}

func sink(out chan<- int, v int) { out <- v }

func drain(in <-chan int) int {
	n := 0
	for v := range in {
		n += v
	}
	return n
}

type queue chan string

func main() {
	ch := make(chan int, 3)
	fmt.Println(len(ch), cap(ch))
	ch <- 1
	ch <- 2
	fmt.Println(len(ch), cap(ch))
	<-ch
	fmt.Println(len(ch), cap(ch))
	u := make(chan string)
	fmt.Println(len(u), cap(u))
	var nc chan int
	fmt.Println(len(nc), cap(nc))
	g := gen(4)
	fmt.Println(len(g), cap(g))
	fmt.Println(drain(g))
	o := make(chan int, 2)
	sink(o, 7)
	fmt.Println(len(o), <-o, len(o))
	var ro <-chan int = o
	fmt.Println(cap(ro))
	q := make(queue, 5)
	q <- "a"
	fmt.Println(len(q), cap(q))
	done := make(chan bool)
	go func(out chan<- bool) { out <- true }(done)
	fmt.Println(<-done)
}
