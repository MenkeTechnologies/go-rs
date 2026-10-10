// Closing a closed or nil channel, and sending on a closed one, are run-time
// panics a deferred `recover` receives as `runtime.Error` values — from a
// plain send and from a `select` send case alike.
package main

import "fmt"

func try(name string, f func()) {
	defer func() {
		r := recover()
		err, isErr := r.(error)
		fmt.Println(name, r, isErr, err)
	}()
	f()
}

func main() {
	ch := make(chan int, 1)
	close(ch)
	try("close closed", func() { close(ch) })
	try("send closed", func() { ch <- 1 })
	try("select send closed", func() {
		select {
		case ch <- 1:
		default:
		}
	})
	var nc chan int
	try("close nil", func() { close(nc) })
	v, ok := <-ch
	fmt.Println(v, ok)
	b := make(chan int, 2)
	b <- 7
	close(b)
	v, ok = <-b
	fmt.Println(v, ok)
	v, ok = <-b
	fmt.Println(v, ok)
	done := make(chan bool)
	go func() {
		defer func() { fmt.Println("goroutine", recover()); done <- true }()
		c := make(chan string)
		close(c)
		close(c)
	}()
	<-done
}
