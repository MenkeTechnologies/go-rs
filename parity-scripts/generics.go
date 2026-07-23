// Generics: type parameters are erased onto the dynamic fusevm value model.
// Exercises generic functions (Sum/Map/Filter/Max over int and float64), a
// constraint interface (~int | ~float64), a generic struct with a pointer
// receiver method, and both inferred and explicit instantiation.
package main

import "fmt"

type Number interface {
	~int | ~float64
}

func Sum[T Number](xs []T) T {
	var total T
	for _, x := range xs {
		total += x
	}
	return total
}

func Map[T any, U any](xs []T, f func(T) U) []U {
	out := make([]U, 0)
	for _, x := range xs {
		out = append(out, f(x))
	}
	return out
}

func Filter[T any](xs []T, keep func(T) bool) []T {
	out := make([]T, 0)
	for _, x := range xs {
		if keep(x) {
			out = append(out, x)
		}
	}
	return out
}

func Max[T Number](a, b T) T {
	if a > b {
		return a
	}
	return b
}

type Stack[T any] struct {
	items []T
}

func (s *Stack[T]) Push(x T) {
	s.items = append(s.items, x)
}

func (s *Stack[T]) Top() T {
	return s.items[len(s.items)-1]
}

func (s *Stack[T]) Len() int {
	return len(s.items)
}

func main() {
	ints := []int{5, 3, 8, 1, 9, 2}
	fmt.Println(Sum(ints))
	fmt.Println(Sum([]float64{1.5, 2.5, 3.0}))
	fmt.Println(Map(ints, func(n int) int { return n * n }))
	fmt.Println(Filter(ints, func(n int) bool { return n > 3 }))
	fmt.Println(Max(3, 9), Max(2.5, 7.5))

	var st Stack[string]
	st.Push("a")
	st.Push("b")
	st.Push("c")
	fmt.Println(st.Len())
	fmt.Println(st.Top())
}
