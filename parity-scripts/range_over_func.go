// Range over a Go 1.23 iterator function: the body runs as the yield
// callback, break stops the iteration (the iterator sees yield return
// false), continue moves to the next value, and a return inside the loop
// returns from the enclosing function once the iterator has stopped.
package main

import "fmt"

func Count(n int) func(func(int) bool) {
	return func(yield func(int) bool) {
		for i := 0; i < n; i++ {
			if !yield(i) {
				fmt.Println("  stopped at", i)
				return
			}
		}
		fmt.Println("  exhausted")
	}
}

func Enumerate[T any](xs []T) func(func(int, T) bool) {
	return func(yield func(int, T) bool) {
		for i, x := range xs {
			if !yield(i, x) {
				return
			}
		}
	}
}

type List struct{ items []string }

func (l *List) All() func(func(string) bool) {
	return func(yield func(string) bool) {
		for _, s := range l.items {
			if !yield(s) {
				return
			}
		}
	}
}

type Seq[V any] func(yield func(V) bool)

func Evens(n int) Seq[int] {
	return func(yield func(int) bool) {
		for i := 0; i < n; i += 2 {
			if !yield(i) {
				return
			}
		}
	}
}

func digits(yield func(int) bool) {
	for _, d := range []int{3, 1, 4} {
		if !yield(d) {
			return
		}
	}
}

type Bag struct{ xs []int }

func (b Bag) Each(yield func(int) bool) {
	for _, x := range b.xs {
		if !yield(x) {
			return
		}
	}
}

func firstOver(limit int) (int, bool) {
	for v := range Count(100) {
		if v*v > limit {
			return v, true
		}
	}
	return -1, false
}

func find(l *List, want string) string {
	for s := range l.All() {
		switch s {
		case "skip":
			continue
		case want:
			return "found " + s
		}
	}
	return "none"
}

func sumBelow(n, stop int) (s int) {
	count := 0
	for v := range Count(n) {
		if v == stop {
			break
		}
		s += v
		count++
	}
	fmt.Println("  counted", count)
	return
}

func main() {
	fmt.Println(sumBelow(10, 4), sumBelow(3, 99))
	for v := range Count(3) {
		fmt.Println("v", v)
	}
	for v := range Count(10) {
		if v == 2 {
			continue
		}
		if v == 4 {
			break
		}
		fmt.Println("w", v)
	}
	fmt.Println(firstOver(30))
	fmt.Println(firstOver(1 << 20))
	for i, s := range Enumerate([]string{"a", "b", "c"}) {
		fmt.Print(i, s, " ")
	}
	fmt.Println()
	l := &List{[]string{"x", "skip", "y", "z"}}
	fmt.Println(find(l, "y"), find(l, "q"))
	total := 0
	for v := range Count(5) {
		for j := 0; j < 10; j++ {
			if j == v {
				break
			}
			total += j
		}
	}
	fmt.Println("total", total)
	var last int
	for last = range Count(4) {
	}
	fmt.Println("last", last)
	seq := Count(3)
	sum := 0
	for v := range seq {
		sum += v
	}
	fmt.Println("sum", sum)
	n := 0
	for range Evens(7) {
		n++
	}
	fmt.Println("evens", n)
	for x := range (Bag{[]int{7, 8}}).Each {
		fmt.Print(x, " ")
	}
	bag := Bag{[]int{1, 2, 3}}
	for x := range bag.Each {
		if x == 3 {
			break
		}
		fmt.Print(x, " ")
	}
	fmt.Println()
	for d := range digits {
		fmt.Print(d)
	}
	fmt.Println()
}
