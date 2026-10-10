// `&x` on a scalar variable, a struct field or a slice/array element is a real
// pointer: reads, `*p = v`, `*p += v`, `**pp`, `new(T)` and pointer identity
// all go through it, including from a closure and a function parameter.
package main

import "fmt"

type S struct {
	n int
	f float64
	s string
}

func bump(p *int) { *p++ }
func swap(a, b *int) { *a, *b = *b, *a }

var g = 10

func main() {
	s := S{1, 2.5, "x"}
	p := &s.n
	*p = 7
	bump(&s.n)
	fmt.Println(s.n, *p)
	arr := []int{1, 2, 3}
	q := &arr[1]
	*q = 20
	bump(&arr[2])
	fmt.Println(arr, *q)
	a, b := 1, 2
	swap(&a, &b)
	fmt.Println(a, b)
	np := new(int)
	*np = 5
	*np += 3
	fmt.Println(*np)
	bump(&g)
	fmt.Println(g)
	pp := &a
	ppp := &pp
	**ppp = 100
	fmt.Println(a)
	fmt.Println(pp == &a, &a == &b, pp != nil)
	var nilp *int
	fmt.Println(nilp == nil)
	fs := &s.f
	*fs *= 2
	ss := &s.s
	*ss += "yz"
	fmt.Println(s)
	m := map[string]*int{}
	x := 3
	m["x"] = &x
	*m["x"]++
	fmt.Println(x)
	cnt := 0
	inc := func() { cnt++ }
	pc := &cnt
	inc()
	*pc += 10
	fmt.Println(cnt, *pc)
	var arr2 [3]int
	pa := &arr2[0]
	*pa = 9
	fmt.Println(arr2)
	for i := 0; i < 3; i++ {
		v := i * 2
		pv := &v
		*pv += 1
		fmt.Print(v, " ")
	}
	fmt.Println()
}
