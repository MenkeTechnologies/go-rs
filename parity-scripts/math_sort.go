package main

import (
	"fmt"
	"math"
	"sort"
)

func main() {
	fmt.Printf("%.4f %.4f %.0f %.0f %.4f\n", math.Sqrt(2), math.Pow(2, 10), math.Floor(3.7), math.Ceil(3.2), math.Abs(-5.5))
	fmt.Println(math.Max(3, 7), math.Min(3, 7))
	fmt.Println(min(5, 2, 8), max(5, 2, 8))
	xs := []int{5, 2, 8, 1, 9, 3}
	sort.Ints(xs)
	fmt.Println(xs)
	ss := []string{"banana", "apple", "cherry"}
	sort.Strings(ss)
	fmt.Println(ss)
}
