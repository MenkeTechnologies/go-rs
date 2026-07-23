// Recoverable runtime panics: a divide-by-zero, an index-out-of-range, and a
// nil dereference are caught by recover() in a deferred closure, which yields
// the Go `runtime error: …` value. A normal path is unaffected.
package main

import "fmt"

type Box struct {
	items []int
}

func guard(f func() int) (result int, err string) {
	defer func() {
		if r := recover(); r != nil {
			err = fmt.Sprint(r)
		}
	}()
	return f(), "ok"
}

func show(v int, err string) {
	fmt.Println(v, err)
}

func main() {
	v0, e0 := guard(func() int { return 42 })
	show(v0, e0)
	v1, e1 := guard(func() int { a, b := 10, 0; return a / b })
	show(v1, e1)
	v2, e2 := guard(func() int { xs := []int{1, 2, 3}; return xs[7] })
	show(v2, e2)
	v3, e3 := guard(func() int { a, b := 17, 5; return a % b })
	show(v3, e3)
	v4, e4 := guard(func() int {
		var box *Box
		return len(box.items)
	})
	show(v4, e4)
	fmt.Println("survived all")
}
