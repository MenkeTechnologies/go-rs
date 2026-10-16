package main

import "fmt"

func main() {
	x := []int{1, 2, 3}
	i := 0
	i, x[i] = 2, 9
	fmt.Println(i, x)

	j := 1
	x[j], j = 7, 2
	fmt.Println(j, x)

	m := map[string]int{}
	k := "a"
	k, m[k] = "b", 5
	fmt.Println(k, m)

	g := [][]int{{0, 0}, {0, 0}}
	r, c := 0, 1
	r, c, g[r][c] = 1, 0, 4
	fmt.Println(r, c, g)

	// a swap through the index being reassigned
	a := []int{10, 20, 30}
	p := 0
	p, a[p], a[p+1] = 1, a[p+1], a[p]
	fmt.Println(p, a)
}
