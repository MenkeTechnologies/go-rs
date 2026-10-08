// A `for … := range` or `case v := <-ch` in a function declares its own
// variables: they never write a package variable, or one of `main`'s, of the
// same name. `case v = <-ch` assigns the variable already in scope.
package main

import "fmt"

func f(a any, ch chan int, m map[string]int) int {
	switch v := a.(type) {
	case int:
		_ = v
	}
	select {
	case w := <-ch:
		_ = w
	default:
	}
	if u, ok := m["k"]; ok {
		_ = u
	}
	for k := range m {
		_ = k
	}
	for c := range ch {
		_ = c
	}
	var t = 5
	r, s := 1, 2
	func() {
		q := 3
		_ = q
	}()
	return t + r + s
}

func main() {
	v, w, u, ok, k, c, t, r, s, q := "v", "w", "u", "ok", "k", "c", "t", "r", "s", "q"
	ch := make(chan int, 2)
	ch <- 7
	ch <- 8
	close(ch)
	fmt.Println(f(1, ch, map[string]int{"k": 1}))
	fmt.Println(v, w, u, ok, k, c, t, r, s, q)
	main3()
}

var total3 int

func sum(xs []int) int {
	var x int
	for _, x = range xs {
		total3 += x
	}
	return x
}

func capture() []func() int {
	var fs []func() int
	for i, v := range []int{10, 20, 30} {
		fs = append(fs, func() int { return i + v })
	}
	return fs
}

func recv(ch chan string) string {
	v := "none"
	select {
	case v = <-ch:
	default:
	}
	return v
}

func main3() {
	x, i, v := "X", "I", "V"
	fmt.Println(sum([]int{1, 2, 3}), total3)
	for _, f := range capture() {
		fmt.Print(f(), " ")
	}
	fmt.Println()
	ch := make(chan string, 1)
	ch <- "got"
	fmt.Println(recv(ch), recv(ch))
	for i, x := range []string{"a", "b"} {
		_, _ = i, x
	}
	fmt.Println(x, i, v)
}
