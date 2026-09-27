package main

import "fmt"

func Clone[S ~[]E, E any](s S) S {
	if s == nil {
		return nil
	}
	return append(S{}, s...)
}

func Keys[M ~map[K]V, K comparable, V any](m M) M {
	out := M{}
	for k, v := range m {
		out[k] = v
	}
	return out
}

func apply(f func(int) int, x int) int { return f(x) }

func ignore(int, string) string { return "ignored" }

func main() {
	a := []int{1, 2, 3}
	b := Clone(a)
	b[0] = 9
	fmt.Println(a, b, len(Clone([]string{})), Clone([]int(nil)) == nil)
	m := Keys(map[string]int{"x": 1})
	m["y"] = 2
	fmt.Println(len(m), m["x"], m["y"])
	fmt.Println(apply(func(n int) int { return n * 2 }, 21), ignore(1, "a"))
}
