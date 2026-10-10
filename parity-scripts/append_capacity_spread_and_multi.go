// A spread append and an append of more elements than the buffer holds allocate;
// one that fits an empty, non-escaping slice lands in the stack buffer.
package main

import "fmt"

func main() {
	xs := []int{1, 2, 3}
	var a []int
	a = append(a, xs...)
	fmt.Println("spread3", cap(a))
	var b []int
	b = append(b, 1, 2)
	fmt.Println("multi2", cap(b))
	var c []byte
	c = append(c, "hi"...)
	fmt.Println("strspread2", cap(c))
	var d []byte
	d = append(d, 'a', 'b', 'c')
	fmt.Println("bytes3", cap(d))
	var e []int
	e = append(e, 1, 2, 3, 4, 5)
	fmt.Println("multi5", cap(e))
	var f []int
	e2 := []int{1, 2, 3, 4}
	f = append(f, e2...)
	fmt.Println("spread4", cap(f))
	var g []int
	g2 := []int{1, 2, 3, 4, 5}
	g = append(g, g2...)
	fmt.Println("spread5", cap(g))
	var h []string
	h = append(h, "a", "b", "c")
	fmt.Println("str3", cap(h))
	var i []int
	i = append(i[:0], 1)
	fmt.Println("slice0", cap(i))
	var j []int
	j = append(j, 1)
	j = append(j, 2, 3, 4, 5, 6)
	fmt.Println("grow-multi", cap(j))
	k := make([]int, 0, 0)
	k = append(k, 1)
	fmt.Println("make00", cap(k))
	var l [][]int
	l = append(l, []int{1})
	l = append(l, []int{2})
	l = append(l, []int{3})
	fmt.Println("2d", cap(l))
}
