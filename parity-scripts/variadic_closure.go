package main

import "fmt"

// A variadic *closure* binds its trailing arguments to a slice, the same way a
// declared variadic function does — through every form that can reach one: a
// call by name, an immediately-invoked literal, a spread, and `go`.

func named(tag string, ns ...int) string { return fmt.Sprint(tag, len(ns), ns) }

func main() {
	// Called by name, with zero, one and several trailing arguments.
	p := func(tag string, a ...any) { fmt.Println(tag, len(a), a) }
	p("none")
	p("one", 1)
	p("many", 1, "x", true)

	// No fixed parameters at all: every argument is packed.
	only := func(ns ...int) int { return len(ns) }
	fmt.Println(only(), only(1), only(1, 2, 3))

	// A spread passes the slice straight through, with and without a fixed head.
	nums := []int{5, 6, 7}
	fmt.Println(only(nums...))
	head := func(tag string, ns ...int) string { return fmt.Sprint(tag, ns) }
	fmt.Println(head("spread", nums...))

	// An immediately-invoked variadic literal.
	func(pre string, xs ...string) { fmt.Println(pre, len(xs), xs) }("iife", "a", "b")

	// The trailing parameter is a slice inside the body, so it ranges and
	// appends like one.
	total := func(ns ...int) int {
		acc := 0
		for _, n := range append(ns, 10) {
			acc += n
		}
		return acc
	}
	fmt.Println(total(), total(1, 2), total(nums...))

	// `go` on a variadic closure and on a variadic declared function. The
	// channel orders the two results, so the output is deterministic.
	out := make(chan string)
	go func(tag string, ns ...int) { out <- fmt.Sprint(tag, len(ns), ns) }("go-lit", 1, 2)
	fmt.Println(<-out)
	g := func(tag string, ns ...int) { out <- fmt.Sprint(tag, len(ns), ns) }
	go g("go-var", 3)
	fmt.Println(<-out)
	go func() { out <- named("go-named", 4, 5, 6) }()
	fmt.Println(<-out)
}
