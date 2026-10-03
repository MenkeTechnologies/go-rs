package main

import (
	"cmp"
	"fmt"
	"maps"
	"slices"
	"strings"
)

type person struct {
	Name string
	Age  int
}

func main() {
	s := []int{5, 2, 8, 2, 9, 1}
	t := slices.Clone(s)
	slices.Sort(t)
	fmt.Println(s, t, slices.IsSorted(t), slices.IsSorted(s))
	i, found := slices.BinarySearch(t, 8)
	fmt.Println(i, found)
	i, found = slices.BinarySearch(t, 7)
	fmt.Println(i, found)
	fmt.Println(slices.Index(s, 8), slices.Index(s, 42), slices.Contains(s, 9))
	fmt.Println(slices.Max(s), slices.Min(s), slices.Equal(s, t), slices.Equal(t, slices.Clone(t)))
	fmt.Println(slices.Compact(slices.Clone(t)))
	r := slices.Clone(s)
	slices.Reverse(r)
	fmt.Println(r)
	fmt.Println(slices.Insert([]int{1, 2, 3}, 1, 10, 11))
	fmt.Println(slices.Delete([]int{1, 2, 3, 4, 5}, 1, 3))
	fmt.Println(slices.Replace([]int{1, 2, 3, 4}, 1, 3, 7, 8, 9))
	fmt.Println(slices.IndexFunc(s, func(v int) bool { return v > 6 }))
	fmt.Println(slices.ContainsFunc(s, func(v int) bool { return v < 0 }))
	fmt.Println(slices.DeleteFunc(slices.Clone(s), func(v int) bool { return v%2 == 0 }))
	fmt.Println(slices.Compare([]int{1, 2}, []int{1, 3}), slices.Compare([]int{1, 2}, []int{1, 2}))
	fmt.Println(slices.Repeat([]string{"a", "b"}, 3), slices.Concat([]int{1}, []int{2, 3}, nil, []int{4}))
	ps := []person{{"Al", 30}, {"Bo", 25}, {"Cy", 30}, {"Di", 22}}
	slices.SortFunc(ps, func(a, b person) int { return cmp.Compare(a.Age, b.Age) })
	fmt.Println(ps)
	slices.SortStableFunc(ps, func(a, b person) int { return strings.Compare(b.Name, a.Name) })
	fmt.Println(ps)
	fmt.Println(slices.MaxFunc(ps, func(a, b person) int { return cmp.Compare(a.Age, b.Age) }))
	j, ok := slices.BinarySearchFunc([]person{{"a", 1}, {"b", 5}}, 5, func(p person, t int) int { return cmp.Compare(p.Age, t) })
	fmt.Println(j, ok)
	words := []string{"pear", "apple", "fig"}
	slices.Sort(words)
	fmt.Println(words, len(slices.Grow(words, 10)), slices.Clip(words))
	for i, w := range slices.All(words) {
		fmt.Print(i, w, " ")
	}
	fmt.Println()
	for v := range slices.Values(words) {
		fmt.Print(v, ";")
	}
	fmt.Println()
	for i, v := range slices.Backward(words) {
		fmt.Print(i, v, ";")
	}
	fmt.Println()
	fmt.Println(slices.Collect(slices.Values([]int{4, 5})), slices.Sorted(slices.Values([]int{3, 1, 2})))
	fmt.Println(slices.AppendSeq([]int{0}, slices.Values([]int{1, 2})))
	for c := range slices.Chunk([]int{1, 2, 3, 4, 5}, 2) {
		fmt.Print(c, " ")
	}
	fmt.Println()

	m := map[string]int{"b": 2, "a": 1, "c": 3}
	keys := slices.Sorted(maps.Keys(m))
	vals := slices.Sorted(maps.Values(m))
	fmt.Println(keys, vals)
	m2 := maps.Clone(m)
	m2["d"] = 4
	fmt.Println(len(m), len(m2), maps.Equal(m, m2))
	delete(m2, "d")
	fmt.Println(maps.Equal(m, m2))
	maps.DeleteFunc(m2, func(k string, v int) bool { return v > 1 })
	fmt.Println(m2)
	maps.Copy(m2, map[string]int{"z": 26})
	fmt.Println(m2)
	m3 := maps.Collect(maps.All(m))
	fmt.Println(m3)
	maps.Insert(m3, maps.All(map[string]int{"q": 9}))
	fmt.Println(m3, maps.EqualFunc(m, m, func(a, b int) bool { return a == b }))
}
