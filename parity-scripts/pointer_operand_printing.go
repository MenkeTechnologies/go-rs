package main

import (
	"errors"
	"fmt"
	"strconv"
)

// A pointer operand to a struct, array, slice or map prints as `&` and the
// value it points at, and `%T` names it `*T` — whether it came from `&T{…}`,
// `new(T)` or `&x`. `*p` is the value again. (A pointer nested inside a printed
// value is a hex address in Go, which no two runs reproduce, so none appears.)

type T struct {
	A int
	B string
}

type Grid [2]int

type Counter struct{ n int }

func (c *Counter) Inc() { c.n++ }

type W int

func (w W) String() string { return fmt.Sprintf("W%d", int(w)) }

func set(p *Grid) { p[0] = 42 }

func show(v any) { fmt.Println(v) }

func main() {
	p := &T{1, "a"}
	q := new(T)
	x := T{2, "b"}
	r := &x
	fmt.Printf("%T %T %T %T\n", p, q, x, r)
	fmt.Println(p, q, x, r, *r)
	fmt.Printf("%v %+v %#v %v\n", p, p, p, *p)
	fmt.Printf("%d %s\n", &T{3, "z"}, &[]string{"a"})
	r.A = 6
	fmt.Println(x, r)

	e1 := errors.New("a")
	e2 := fmt.Errorf("plain %d", 1)
	e3 := fmt.Errorf("w %w", e1)
	e4 := fmt.Errorf("w %w %w", e1, e2)
	fmt.Printf("%T %T %T %T\n", e1, e2, e3, e4)
	_, err := strconv.Atoi("x")
	fmt.Printf("%T %T\n", err, errors.Unwrap(err))

	m := map[string]int{"k": 1}
	a := [2]int{3, 4}
	s := []int{5}
	fmt.Println(&m, &a, &s)
	pa := &a
	pa[0] = 9
	fmt.Println(pa, a, *pa)

	g := Grid{1, 2}
	pg := &g
	set(pg)
	fmt.Println(pg, g)
	fmt.Printf("%T %T\n", pg, *pg)
	var i any = pg
	fmt.Printf("%v %T\n", i, i)
	show(pg)
	show(*pg)

	c := &Counter{}
	c.Inc()
	(*c).Inc()
	fmt.Println(c, *c, c.n)
	ps := []*T{{1, "a"}}
	fmt.Printf("%T %T %v\n", ps, ps[0], *ps[0])
	var iface interface{} = &T{}
	switch v := iface.(type) {
	case *T:
		fmt.Printf("ptr %T\n", v)
	case T:
		fmt.Println("val")
	}
	fmt.Println(fmt.Sprint(&T{9, "q"}), W(4), fmt.Sprintf("%v", c))
}
