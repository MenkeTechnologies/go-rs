// Package initialization order: package-level variables initialize in
// dependency order — not source order — with a function's body counting as
// part of every initializer that calls it, and then each `init` function runs,
// in source order, before `main`. A package may declare several `init`s.
package main

import "fmt"

var trace []string

func note(s string, v int) int {
	trace = append(trace, s)
	return v
}

// `a` needs `b`, which needs `f`, which reads `c`: the order is c, b, a.
var a = note("a", b+1)
var b = note("b", f())

func f() int { return c * 10 }

var c = note("c", 2)

// A group is ordered per variable, not per group.
var (
	x = note("x", y*2)
	y = note("y", 5)
)

// A variable nobody depends on keeps its source position.
var z = note("z", 0)

// Declared after its use.
var total = sum(nums)
var nums = []int{1, 2, 3}

func sum(xs []int) int {
	t := 0
	for _, v := range xs {
		t += v
	}
	return t
}

func init() {
	trace = append(trace, "init1")
	total *= 10
}

func init() {
	trace = append(trace, fmt.Sprint("init2 total=", total))
}

func main() {
	fmt.Println(trace)
	fmt.Println(a, b, c, x, y, z, total)
}
