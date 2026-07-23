package main

import "fmt"

type Rect struct {
	w int
	h int
}

func (r Rect) area() int {
	return r.w * r.h
}

func main() {
	r := Rect{w: 3, h: 4}
	fmt.Println("area", r.area())

	// slices + range
	nums := []int{5, 2, 8, 1}
	max := nums[0]
	for _, n := range nums {
		if n > max {
			max = n
		}
	}
	fmt.Println("max", max)

	// maps
	counts := map[string]int{}
	for _, w := range []string{"a", "b", "a", "c", "a"} {
		counts[w]++
	}
	fmt.Println("counts", counts)
}
