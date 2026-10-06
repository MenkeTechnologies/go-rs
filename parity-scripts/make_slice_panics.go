package main

// make([]T, len, cap) panics the way runtime.makeslice does: a negative
// length first, then a capacity below the length (or negative), each a
// recoverable runtime error.

import "fmt"

func try(name string, f func()) {
	defer func() {
		if r := recover(); r != nil {
			fmt.Println(name, "->", r)
		}
	}()
	f()
	fmt.Println(name, "ok")
}

func main() {
	n, neg := 3, -1
	try("len -1", func() { _ = make([]int, neg) })
	try("cap -1", func() { _ = make([]int, 0, neg) })
	try("cap < len", func() { _ = make([]int, n, n-1) })
	try("ok", func() { s := make([]string, n, n+2); fmt.Println(len(s), cap(s)) })
	try("both bad", func() { _ = make([]int, neg, neg) })
}
