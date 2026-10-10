// Which standard-library calls keep a slice argument (sort.Slice, fmt, a channel
// send, a map store, `&s`) and which do not (sort.Ints, strings.Join, slices.*,
// errors.Join), seen through the capacity of the slice they were handed.
package main

import (
	"bytes"
	"errors"
	"fmt"
	"slices"
	"sort"
	"strings"
)

func main() {
	var a []int
	a = append(a, 1)
	sort.Ints(a)
	fmt.Println("sort.Ints", cap(a))
	var b []string
	b = append(b, "x")
	sort.Strings(b)
	fmt.Println("sort.Strings", cap(b))
	var c []float64
	c = append(c, 1)
	sort.Float64s(c)
	fmt.Println("sort.Float64s", cap(c))
	var d []int
	d = append(d, 1)
	sort.Slice(d, func(i, j int) bool { return d[i] < d[j] })
	fmt.Println("sort.Slice", cap(d))
	var e []int
	e = append(e, 1)
	_ = sort.SearchInts(e, 1)
	fmt.Println("sort.SearchInts", cap(e))
	var f []string
	f = append(f, "x")
	_ = strings.Join(f, ",")
	fmt.Println("strings.Join", cap(f))
	var g []int
	g = append(g, 1)
	_ = slices.Max(g)
	fmt.Println("slices.Max", cap(g))
	var h []int
	h = append(h, 1)
	slices.Reverse(h)
	fmt.Println("slices.Reverse", cap(h))
	var i []int
	i = append(i, 1)
	_ = slices.Clone(i)
	fmt.Println("slices.Clone", cap(i))
	var j []int
	j = append(j, 1)
	_ = slices.Equal(j, j)
	fmt.Println("slices.Equal", cap(j))
	var k []byte
	k = append(k, 1)
	_ = bytes.Contains(k, k)
	fmt.Println("bytes.Contains", cap(k))
	var l []error
	l = append(l, nil)
	_ = errors.Join(l...)
	fmt.Println("errors.Join", cap(l))
	var m []int
	m = append(m, 1)
	_ = fmt.Sprint(len(m))
	fmt.Println("fmt.Sprint(len)", cap(m))
	var n []int
	n = append(n, 1)
	_ = fmt.Sprintf("%d", n)
	fmt.Println("fmt.Sprintf", cap(n))
	var o []int
	o = append(o, 1)
	ch := make(chan []int, 1)
	ch <- o
	fmt.Println("chan", cap(o))
	var p []int
	p = append(p, 1)
	go func() { _ = p }()
	fmt.Println("go-closure", cap(p))
	var q []int
	q = append(q, 1)
	defer func() { _ = q }()
	fmt.Println("defer-closure", cap(q))
	var r []int
	r = append(r, 1)
	mm := map[string][]int{}
	mm["a"] = r
	fmt.Println("map-store", cap(r), len(mm))
	var s []int
	s = append(s, 1)
	pp := &s
	fmt.Println("addr", cap(s), len(*pp))
	var t []int
	t = append(t, 1)
	u := [][]int{t}
	fmt.Println("nested", cap(t), len(u))
	var v []int
	v = append(v, 1)
	var iface interface{} = v
	_ = iface
	fmt.Println("iface", cap(v))
	var w []int
	w = append(w, 1)
	x := struct{ s []int }{w}
	fmt.Println("anon-struct", cap(w), len(x.s))
	var y []int
	y = append(y, 1)
	for _, z := range y {
		_ = z
	}
	fmt.Println("range", cap(y))
}
