// A variadic method packs its trailing arguments into the slice its last
// parameter binds — none, several, or a slice spread with `xs...` — through
// a pointer or a value receiver, as a variadic function does.
package main

import (
	"fmt"
	"strings"
)

type Log struct{ lines []string }

func (l *Log) Add(level string, parts ...string) int {
	l.lines = append(l.lines, level+": "+strings.Join(parts, " "))
	return len(parts)
}

type Sum struct{ base int }

func (s Sum) Of(xs ...int) int {
	t := s.base
	for _, x := range xs {
		t += x
	}
	return t
}

func (s Sum) Count(label string, xs ...int) string {
	return fmt.Sprint(label, len(xs), xs == nil)
}

func none(xs ...int) bool { return xs == nil }

func main() {
	l := &Log{}
	fmt.Println(l.Add("info", "a", "b", "c"))
	fmt.Println(l.Add("warn"))
	words := []string{"x", "y"}
	fmt.Println(l.Add("err", words...))
	fmt.Println(strings.Join(l.lines, " | "))
	s := Sum{10}
	fmt.Println(s.Of(), s.Of(1), s.Of(1, 2, 3), s.Of([]int{4, 5}...))
	fmt.Println(s.Count("none"), s.Count("two", 1, 2))
	var lg Log
	lg.Add("v", "q")
	fmt.Println(lg.lines)
	fmt.Println(none(), none(1), none([]int{}...))
	h := func(p string, xs ...string) bool { return xs == nil }
	fmt.Println(h("a"), h("a", "b"))
	var ns []int
	t := append(ns)
	u := append([]int{1, 2}[:1])
	fmt.Println(t == nil, len(u), cap(u))
}
