
// sort's binary searches and its already-sorted predicates. A search returns
// the INSERTION POINT, so a miss comes back as the index where the value would
// go — `len(a)` past the end — and never as a negative. `sort.Search` itself is
// the general form the typed three are written on: the smallest index whose
// predicate is true, or n when none is.
package main

import (
	"fmt"
	"sort"
)

func main() {
	a := []int{1, 3, 5, 7}
	fmt.Println(sort.SearchInts(a, 3), sort.SearchInts(a, 4), sort.SearchInts(a, 0), sort.SearchInts(a, 9))
	fmt.Println(sort.SearchInts([]int{}, 1))
	s := []string{"a", "c", "e"}
	fmt.Println(sort.SearchStrings(s, "c"), sort.SearchStrings(s, "b"), sort.SearchStrings(s, "z"))
	f := []float64{1.5, 2.5}
	fmt.Println(sort.SearchFloat64s(f, 2.5), sort.SearchFloat64s(f, 9))
	fmt.Println(sort.Search(5, func(i int) bool { return i >= 3 }))
	fmt.Println(sort.Search(5, func(i int) bool { return false }))
	fmt.Println(sort.Search(0, func(i int) bool { return true }))
	fmt.Println(sort.IntsAreSorted([]int{1, 2, 2}), sort.IntsAreSorted([]int{2, 1}))
	fmt.Println(sort.StringsAreSorted([]string{"a", "b"}), sort.StringsAreSorted([]string{"b", "a"}))
	fmt.Println(sort.Float64sAreSorted([]float64{1, 2}), sort.Float64sAreSorted([]float64{2, 1}))
	fmt.Println(sort.IntsAreSorted([]int{}), sort.IntsAreSorted([]int{9}))
	xs := []int{3, 1, 2}
	fmt.Println(sort.SliceIsSorted(xs, func(i, j int) bool { return xs[i] < xs[j] }))
	sort.Slice(xs, func(i, j int) bool { return xs[i] < xs[j] })
	fmt.Println(xs, sort.SliceIsSorted(xs, func(i, j int) bool { return xs[i] < xs[j] }))
}
