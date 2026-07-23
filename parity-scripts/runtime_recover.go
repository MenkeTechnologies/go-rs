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

func main() {
	// guard returns (int, string); pass both straight to Println (multi-value
	// spread).
	fmt.Println(guard(func() int { return 42 }))
	fmt.Println(guard(func() int { a, b := 10, 0; return a / b }))
	fmt.Println(guard(func() int { xs := []int{1, 2, 3}; return xs[7] }))
	fmt.Println(guard(func() int { a, b := 17, 5; return a % b }))
	fmt.Println(guard(func() int {
		var box *Box
		return len(box.items)
	}))
	fmt.Println("survived all")
}
