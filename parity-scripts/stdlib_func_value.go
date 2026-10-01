// A natively implemented stdlib function used as a value: bound to a
// variable, passed as an argument, stored in a slice or map, and called
// through it, with its results (one, two, or none) intact — including the
// synthesized ones that take a closure (strings.Map, sort.Search, …).
package main

import (
	"fmt"
	"math"
	"sort"
	"strconv"
	"strings"
)

func apply(f func(string) string, s string) string { return f(s) }

func mapf(xs []float64, f func(float64) float64) []float64 {
	out := make([]float64, 0, len(xs))
	for _, x := range xs {
		out = append(out, f(x))
	}
	return out
}

func main() {
	up := strings.ToUpper
	fmt.Println(up("abc"))
	fmt.Println(apply(strings.ToLower, "XyZ"))
	fmt.Println(apply(strings.TrimSpace, "  pad  "))
	fmt.Println(mapf([]float64{1, 4, 9}, math.Sqrt))
	ops := map[string]func(float64, float64) float64{"pow": math.Pow, "max": math.Max}
	fmt.Println(ops["pow"](2, 10), ops["max"](3, 7))
	conv := strconv.Atoi
	n, err := conv("42")
	fmt.Println(n, err)
	_, err = conv("x")
	fmt.Println(err)
	pr := fmt.Println
	pr("via", "value", 3)
	sp := fmt.Sprintf
	fmt.Println(sp("%03d|%s", 7, "z"))
	srt := sort.Strings
	words := []string{"pear", "apple", "fig"}
	srt(words)
	fmt.Println(words)
	fs := []func(string, string) bool{strings.HasPrefix, strings.Contains}
	for _, f := range fs {
		fmt.Println(f("golang", "go"), f("golang", "lan"))
	}
	join := strings.Join
	fmt.Println(join([]string{"a", "b"}, "-"))
}

func init() {
	m := strings.Map
	fmt.Println(m(func(r rune) rune { return r + 1 }, "abc"))
	ff := strings.FieldsFunc
	isX := func(r rune) bool { return r == 'x' }
	fmt.Println(ff("axbxxc", isX), len(ff("axbxxc", isX)))
	tf := strings.TrimFunc
	fmt.Println(tf("xxhixx", isX))
	s := sort.Search
	fmt.Println(s(100, func(i int) bool { return i*i >= 50 }))
	si := sort.SearchInts
	fmt.Println(si([]int{1, 3, 5, 7}, 5))
}
