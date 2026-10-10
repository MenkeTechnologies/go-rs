// Go 1.22 gives each iteration of a `for` loop its own copy of the loop
// variable: a closure that writes it, or a pointer to it, keeps that
// iteration's variable — and the post statement runs on the next one.
package main

import (
	"fmt"
	"sync"
)

func main() {
	var fs []func() int
	var ps []*int
	for i := 0; i < 5; i++ {
		fs = append(fs, func() int { i += 2; return i })
		ps = append(ps, &i)
	}
	for _, f := range fs {
		fmt.Print(f(), " ", f(), " ")
	}
	fmt.Println()
	for _, p := range ps {
		fmt.Print(*p, " ")
	}
	fmt.Println()

	var gs []func() int
	for i, j := 0, 10; i < 3; i, j = i+1, j-1 {
		gs = append(gs, func() int { j *= 2; return i*100 + j })
	}
	for _, g := range gs {
		fmt.Print(g(), " ", g(), " ")
	}
	fmt.Println()

	var wg sync.WaitGroup
	out := make([]int, 4)
	for i := range 4 {
		wg.Add(1)
		go func() {
			defer wg.Done()
			i += 100
			out[i-100] = i
		}()
	}
	wg.Wait()
	fmt.Println(out)

	var hs []func() string
	for _, w := range []string{"a", "b", "c"} {
		hs = append(hs, func() string { w += "!"; return w })
	}
	for _, h := range hs {
		fmt.Print(h(), h(), " ")
	}
	fmt.Println()

	var ptrs []*int
	for i := 0; i < 3; i++ {
		ptrs = append(ptrs, &i)
		i++
	}
	for _, p := range ptrs {
		fmt.Print(*p, " ")
	}
	fmt.Println()
}
