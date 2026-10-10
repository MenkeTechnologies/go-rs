// A conversion to a type parameter, the zero value of one, and `make([]T, n)`
// follow the type argument the call site resolved — written (`F[int](x)`) or
// inferred from the arguments, and forwarded through a nested generic call or
// a closure.
package main

import "fmt"

type Number interface {
	~int | ~int64 | ~float64
}

type F float64

func Avg[T Number](xs ...T) T {
	var s T
	for _, x := range xs {
		s += x
	}
	return s / T(len(xs))
}

func Zero[T any]() T {
	var z T
	return z
}

func First[T any](xs []T) T {
	var zero T
	if len(xs) == 0 {
		return zero
	}
	return xs[0]
}

func Conv[T, U Number](x T) U { return U(x) }

func Scale[T Number](xs []T, k int) []T {
	out := make([]T, len(xs))
	for i, x := range xs {
		out[i] = x * T(k)
	}
	return out
}

func Wrap[T Number](x T) T { return Avg(x, x, x) }

func Blank[T any](n int) []T { return make([]T, n) }

func MapAll[T any, U any](xs []T, f func(T) U) []U {
	r := make([]U, 0)
	g := func(x T) U { var z U; _ = z; return f(x) }
	for _, x := range xs {
		r = append(r, g(x))
	}
	return r
}

func main() {
	fmt.Println(Avg(1.0, 2.0, 4.0), Avg(3, 4), Avg(F(1), F(2)))
	fmt.Println(Avg[float64](1, 2), Avg[int](5, 8))
	fmt.Println(Zero[int](), Zero[string]() == "", Zero[float64](), Zero[bool](), Zero[[]int]() == nil)
	fmt.Printf("%q %v %v %v\n", First([]string{}), First([]float64{}), First([]int{}), First([]int{7}))
	fmt.Println(Conv[int, float64](3), Conv[float64, int](3.9))
	fmt.Println(Scale([]float64{1.5, 2}, 2), Scale([]int{1, 2}, 3))
	fmt.Println(Wrap(3.0), Wrap(4))
	fmt.Printf("%q %v\n", Blank[string](2), Blank[float64](2))
	fmt.Println(MapAll([]int{1, 2}, func(i int) string { return fmt.Sprint(i * 2) }))
}
