package main

import (
	"fmt"
	"sort"
	"strings"
)

// A *declared* function named in a value position is a function value, and
// reaches every place a closure does: an argument, a variable, a container, a
// struct field, and a stdlib callback slot.

func dbl(n int) int         { return n * 2 }
func inc(n int) int         { return n + 1 }
func shout(s string) string { return strings.ToUpper(s) }
func announce()             { fmt.Println("announced") }
func tally(label string, ns ...int) string {
	return fmt.Sprint(label, len(ns), ns)
}

type pipeline struct {
	stage func(int) int
	name  string
}

func apply(f func(int) int, v int) int { return f(v) }

func compose(f, g func(int) int) func(int) int {
	return func(n int) int { return f(g(n)) }
}

func main() {
	// As an argument, and returned back out of a higher-order function.
	fmt.Println(apply(dbl, 21), apply(inc, 41))
	fmt.Println(compose(dbl, inc)(4))

	// Bound to a variable — including one declared with `var` and assigned
	// later, which is how a mutually recursive pair is written.
	f := dbl
	fmt.Println(f(5))
	var g func(int) int
	g = inc
	fmt.Println(g(5))

	// In a slice, a map and a struct field.
	fns := []func(int) int{dbl, inc}
	fmt.Println(fns[0](10), fns[1](10))
	byName := map[string]func(int) int{"dbl": dbl, "inc": inc}
	fmt.Println(byName["dbl"](7), byName["inc"](7))
	p := pipeline{stage: dbl, name: "double"}
	stage := p.stage
	fmt.Println(p.name, stage(8))

	// A result-less one, and a non-numeric signature.
	a := announce
	a()
	s := shout
	fmt.Println(s("hi"))

	// A *variadic* declared function keeps its signature as a value: the
	// wrapper's trailing parameter is the packed slice, spread into the call.
	t := tally
	fmt.Println(t("none"), t("two", 1, 2))
	ns := []int{3, 4, 5}
	fmt.Println(t("spread", ns...))

	// A stdlib callback slot that takes a func value.
	words := []string{"pear", "fig", "apple"}
	sort.Slice(words, func(i, j int) bool { return words[i] < words[j] })
	fmt.Println(words)

	// A func value compares against nil, and a local shadows the name.
	var zero func(int) int
	fmt.Println(zero == nil, f == nil)
	dbl := 99
	fmt.Println(dbl)
}
