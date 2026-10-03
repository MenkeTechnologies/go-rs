package main

import (
	"fmt"
	"strings"
)

type pipe struct {
	pair  func(int) (int, int)
	label func(string, ...int) string
}

type handler func(name string, vals ...int) (string, error)

func apply(f func(string, ...int) int) { fmt.Println(f("dyn", 4, 5, 6)) }

func applyNone(f func(string, ...int) int) { fmt.Println(f("none")) }

func mk() func(int) (int, int) {
	return func(n int) (int, int) { return n * 2, n * 3 }
}

func divmod(a, b int) (int, int) { return a / b, a % b }

func use(h handler) {
	s, err := h("h", 1, 2)
	fmt.Println(s, err)
}

func pick(fs []func(int) (string, bool), i, v int) {
	s, ok := fs[i](v)
	fmt.Println(s, ok)
}

func main() {
	apply(func(l string, ns ...int) int { fmt.Println(l, len(ns)); return len(ns) })
	applyNone(func(l string, ns ...int) int { fmt.Println(l, ns == nil); return len(ns) })
	p := pipe{pair: func(n int) (int, int) { return n, n + 1 }}
	a, b := p.pair(4)
	fmt.Println(a, b)
	p.label = func(s string, xs ...int) string { return fmt.Sprint(s, xs) }
	fmt.Println(p.label("L", 1, 2, 3), p.label("E"))
	x, y := mk()(5)
	fmt.Println(x, y)
	g := mk()
	q, r := g(7)
	fmt.Println(q, r)
	var dm func(int, int) (int, int) = divmod
	fmt.Println(dm(17, 5))
	use(func(n string, v ...int) (string, error) { return strings.Repeat(n, len(v)), nil })
	fs := []func(int) (string, bool){func(n int) (string, bool) { return fmt.Sprint(n), n > 0 }}
	pick(fs, 0, -3)
	m := map[string]func(...string) string{"j": func(p ...string) string { return strings.Join(p, "+") }}
	fmt.Println(m["j"]("a", "b", "c"), m["j"]())
	fmt.Printf("%T\n", p.pair)
	fmt.Printf("%T\n", apply)
	fmt.Printf("%T %T\n", mk, mk())
	var h handler
	fmt.Println(h == nil)
	var fv func(float64) float64 = func(v float64) float64 { return v / 2 }
	fmt.Println(fv(3))
}
