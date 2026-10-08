package main

import "fmt"

type Bag struct {
	items []string
	tags  map[string]int
}

func push(p *[]int, x int) { *p = append(*p, x) }

func reset(m *map[string]int) { *m = map[string]int{"fresh": 1} }

func fill(p *[]int, n int) {
	for i := 0; i < n; i++ {
		push(p, i)
	}
	fmt.Println(p, len(*p))
}

func main() {
	var s []int
	t := s
	push(&s, 1)
	push(&s, 2)
	fmt.Println(s, len(s), t == nil)
	u := s
	push(&s, 3)
	fmt.Println(s, u)
	var b Bag
	add := func(p *[]string, v string) { *p = append(*p, v) }
	add(&b.items, "x")
	add(&b.items, "y")
	reset(&b.tags)
	fmt.Println(b.items, b.tags)
	grid := [][]int{{1}, {2}}
	push(&grid[1], 9)
	fmt.Println(grid)
	var acc []int
	fill(&acc, 4)
	fmt.Println(acc)
	m := map[string]int{"a": 1}
	reset(&m)
	fmt.Println(m)
	main2()
}

type Stack struct{ data []int }

func (st *Stack) pushAll(dst *[]int, xs ...int) {
	for _, x := range xs {
		*dst = append(*dst, x)
	}
	st.data = append(st.data, len(xs))
}

func grow(p *[]int) {
	inner := func(q *[]int) { *q = append(*q, 100) }
	inner(p)
}

func main2() {
	s := []int{1}
	ps := &s
	*ps = append(*ps, 2)
	fmt.Println(s, *ps, len(s))
	grow(ps)
	fmt.Println(s)
	var st Stack
	var out []int
	st.pushAll(&out, 7, 8)
	fmt.Println(out, st.data)
	m := map[string]int{}
	pm := &m
	*pm = map[string]int{"z": 26}
	fmt.Println(m, len(*pm))
	pp := &ps
	fmt.Println(len(**pp))
	var e []string
	pe := &e
	*pe = append(*pe, "a")
	*pe = append(*pe, "b")
	fmt.Printf("%v %d %T\n", e, len(e), pe)
}
