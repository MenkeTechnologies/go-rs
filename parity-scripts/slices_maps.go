package main

import "fmt"

func main() {
	xs := []int{5, 3, 8, 1}
	xs = append(xs, 9, 2)
	total := 0
	for _, v := range xs {
		total += v
	}
	fmt.Println(xs, len(xs), total)

	grid := make([]int, 5)
	for i := 0; i < len(grid); i++ {
		grid[i] = i * i
	}
	fmt.Println(grid)

	m := map[string]int{"a": 1, "b": 2}
	m["c"] = 3
	m["a"] = 10
	delete(m, "b")
	fmt.Println(m["a"], m["c"], len(m))
}
