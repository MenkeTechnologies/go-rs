// A variable bound from a call whose declared results are scalar can have its
// address taken: user functions, native stdlib functions, multi-value returns,
// parameters, named results and pointer-to-slice-element writes.
package main

import (
	"fmt"
	"strconv"
	"strings"
)

func two() (int, float64) { return 1, 2.5 }

func named() (n int, s string) {
	p := &n
	*p = 7
	q := &s
	*q = "named"
	return
}

func bump(p *int) { *p++ }

func swap(a, b *string) { *a, *b = *b, *a }

func main() {
	n, err := strconv.Atoi("5")
	p := &n
	*p += 4
	fmt.Println(n, err)
	a, b := two()
	pa, pb := &a, &b
	*pa += 10
	*pb *= 2
	fmt.Println(a, b)
	fmt.Println(named())
	i := strings.Index("hello", "l")
	bump(&i)
	fmt.Println(i)
	x, y := "x", "y"
	swap(&x, &y)
	fmt.Println(x, y)
	f, _ := strconv.ParseFloat("1.5", 64)
	pf := &f
	*pf += 1
	fmt.Println(f)
	count := 0
	inc := func() { count++ }
	pc := &count
	inc()
	inc()
	*pc *= 10
	fmt.Println(count, *pc)
}
