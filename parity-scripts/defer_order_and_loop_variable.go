package main

import "fmt"

func main() {
	// `defer` runs last-in-first-out, and its ARGUMENTS are evaluated where
	// the defer statement runs, not where the call finally happens.
	func() {
		for i := 0; i < 3; i++ {
			defer fmt.Println("defer", i)
		}
		fmt.Println("body done")
	}()
	// A deferred closure can still change a NAMED result after `return`.
	fmt.Println(namedResult())
	// Go 1.22 onward: each iteration gets its own loop variable, so a closure
	// made in the loop captures that iteration's value rather than the last.
	fns := []func() int{}
	for i := 0; i < 3; i++ {
		fns = append(fns, func() int { return i })
	}
	for _, f := range fns {
		fmt.Print(f(), " ")
	}
	fmt.Println()
	// The same for a range loop's key and value.
	var got []string
	for _, s := range []string{"a", "b"} {
		got = append(got, s)
	}
	fmt.Println(got)
}

func namedResult() (n int) {
	defer func() { n *= 10 }()
	n = 4
	return n
}
