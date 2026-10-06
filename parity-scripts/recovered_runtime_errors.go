package main

import (
	"errors"
	"fmt"
	"os"
)

// The value `recover()` returns for a run-time fault is an `error` of the
// runtime's own types; a `panic` of a string stays a string. `os.Exit` ends
// the program at once: no deferred call runs.

type node struct {
	next *node
	val  int
}

func try(name string, f func()) {
	defer func() {
		r := recover()
		err, isErr := r.(error)
		fmt.Printf("%s: %v | %T | error=%v", name, r, r, isErr)
		if isErr {
			fmt.Printf(" | %q", err.Error())
		}
		fmt.Println()
	}()
	f()
}

func main() {
	defer fmt.Println("not printed: os.Exit skips deferred calls")
	try("index", func() {
		var s []int
		_ = s[3]
	})
	try("slice", func() {
		s := []int{1, 2}
		i := 5
		_ = s[1:i]
	})
	try("nil map", func() {
		var m map[string]int
		m["k"] = 1
	})
	try("divide", func() {
		a, b := 1, 0
		_ = a / b
	})
	try("nil deref", func() {
		var n *node
		_ = n.val
	})
	try("assert", func() {
		var v any = "s"
		_ = v.(int)
	})
	try("string", func() { panic("plain") })
	try("error", func() { panic(errors.New("custom")) })
	try("none", func() {})
	fmt.Println(len(os.Args), len(os.Args[1:]))
	fmt.Printf("%T\n", os.Args)
	os.Exit(0)
}
