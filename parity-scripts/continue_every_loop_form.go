package main

// `continue` in every loop form. A mis-targeted continue jump does not print a
// wrong answer — it spins forever, which a byte-diff harness cannot see (a
// process that never exits produces no output to compare). The bounded version
// of this check lives in tests/loop_continue.rs.

import "fmt"

func main() {
	// three-clause for
	s := 0
	for i := 0; i < 6; i++ {
		if i%2 == 0 {
			continue
		}
		s += i
	}
	fmt.Println("three-clause", s)

	// condition-only for
	j, t := 0, 0
	for j < 6 {
		j++
		if j%2 == 0 {
			continue
		}
		t += j
	}
	fmt.Println("cond-only", t)

	// bare for with break
	k, u := 0, 0
	for {
		k++
		if k > 6 {
			break
		}
		if k%2 == 0 {
			continue
		}
		u += k
	}
	fmt.Println("bare", u)

	// range over slice
	r := 0
	for _, v := range []int{1, 2, 3, 4, 5} {
		if v%2 == 0 {
			continue
		}
		r += v
	}
	fmt.Println("range-slice", r)

	// range over map
	m := map[string]int{"a": 1, "b": 2, "c": 3}
	mm := 0
	for _, v := range m {
		if v == 2 {
			continue
		}
		mm += v
	}
	fmt.Println("range-map", mm)

	// range over string
	rs := 0
	for _, c := range "abcde" {
		if c == 'c' {
			continue
		}
		rs += int(c)
	}
	fmt.Println("range-string", rs)

	// range over int (Go 1.22+)
	ri := 0
	for i := range 6 {
		if i%2 == 0 {
			continue
		}
		ri += i
	}
	fmt.Println("range-int", ri)

	// labeled continue, nested
	lc := 0
outer:
	for i := 0; i < 4; i++ {
		for j := 0; j < 4; j++ {
			if j > i {
				continue outer
			}
			lc++
		}
	}
	fmt.Println("labeled", lc)

	// continue as the last statement of the body
	last := 0
	for i := 0; i < 5; i++ {
		last += i
		continue
	}
	fmt.Println("trailing", last)

	// continue inside a switch inside a loop
	sw := 0
	for i := 0; i < 6; i++ {
		switch i % 3 {
		case 0:
			continue
		case 1:
			sw += i
		default:
			sw += 100
		}
	}
	fmt.Println("switch", sw)

	// continue inside a select inside a loop
	ch := make(chan int, 3)
	ch <- 1
	ch <- 2
	ch <- 3
	sel := 0
	for i := 0; i < 3; i++ {
		select {
		case v := <-ch:
			if v == 2 {
				continue
			}
			sel += v
		}
	}
	fmt.Println("select", sel)
}
