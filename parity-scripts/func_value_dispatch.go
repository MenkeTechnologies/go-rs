package main

import (
	"fmt"
	"strconv"
)

// How a call *through* a function value resolves: which lambda it reaches, and
// how many values it yields.

func divmod(a, b int) (int, int) { return a / b, a % b }

func triple(n int) int { return n * 3 }

// `f` here is a parameter. A name bound to a function literal anywhere else in
// the program must not decide what it dispatches to.
func apply(f func(int) int, v int) int { return f(v) }

func twice(f func(int) int, v int) int { return f(f(v)) }

func main() {
	// The same parameter name is bound to a different literal in main.
	f := func(n int) int { return n * 10 }
	fmt.Println(apply(func(n int) int { return n + 1 }, 10))
	fmt.Println(twice(func(n int) int { return n + 1 }, 10))
	fmt.Println(apply(triple, 10), f(10))

	// Multi-value results through a func value: a literal, and a declared
	// function bound to a name.
	lit := func(a, b int) (int, int) { return a + b, a - b }
	s, d := lit(9, 4)
	fmt.Println(s, d)
	dm := divmod
	q, r := dm(17, 5)
	fmt.Println(q, r)

	// A `(value, error)` pair forwarded through a literal.
	parse := func(s string) (int, error) { return strconv.Atoi(s) }
	n, err := parse("42")
	fmt.Println(n, err)
	bad, err2 := parse("x")
	fmt.Println(bad, err2)

	// Three results, and the same call spread into a print.
	three := func() (int, string, bool) { return 1, "two", true }
	a, b, c := three()
	fmt.Println(a, b, c)
	fmt.Println(three())

	// A single-result call still yields one value.
	fmt.Println(lit(1, 1))
}
