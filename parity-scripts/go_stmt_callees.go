package main

import "fmt"

type worker struct{ id int }

func (w *worker) run(out chan string) { out <- fmt.Sprint("w", w.id) }

func spawn(f func(chan string), out chan string) { go f(out) }

type job struct{ do func(int) }

func main() {
	out := make(chan string)
	run := func() { out <- "local" }
	go run()
	fmt.Println(<-out)

	// A method call: the receiver is evaluated when the go statement runs.
	w := &worker{7}
	go w.run(out)
	fmt.Println(<-out)

	// A func-typed parameter, a slice element and a struct field.
	spawn(func(c chan string) { c <- "param" }, out)
	fmt.Println(<-out)
	fs := []func(){func() { out <- "elem" }}
	go fs[0]()
	fmt.Println(<-out)
	j := job{do: func(n int) { out <- fmt.Sprint("field ", n) }}
	go j.do(3)
	fmt.Println(<-out)

	// A captured closure started from inside another closure.
	starter := func() { go run() }
	starter()
	fmt.Println(<-out)

	// Arguments are evaluated at the go statement, not when it runs.
	n := 1
	res := make(chan int)
	add := func(a, b int) { res <- a + b }
	go add(n, n*10)
	n = 100
	fmt.Println(<-res, n)
}
