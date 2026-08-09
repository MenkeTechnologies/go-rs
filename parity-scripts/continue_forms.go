// `continue` in every loop form. Each form backpatches its own jump list at its
// own site, so a form can be correct in one shape and broken in another — an
// unpatched placeholder jumps to instruction 0 (an infinite loop), and a
// three-clause `for` patched to the *test* rather than the *post* clause skips
// the update, which is a silently wrong answer rather than a hang. Every form
// gets its own case here for that reason.
package main

import "fmt"

func main() {
	// (a) three-clause for: `continue` MUST still run the post clause.
	// If it were patched to the test instead, i never increments -> hang,
	// or with a different bound, a silently wrong count.
	n := 0
	for i := 0; i < 6; i++ {
		if i%2 == 0 {
			continue
		}
		n += i
	}
	fmt.Println("three-clause", n)

	// post clause with a side effect that must not be skipped
	steps := 0
	for i := 0; i < 4; i = i + 1 {
		steps++
		if i == 1 {
			continue
		}
	}
	fmt.Println("steps", steps)

	// (b) condition-only for: `continue` goes to the test.
	j := 0
	c := 0
	for j < 6 {
		j++
		if j%2 == 0 {
			continue
		}
		c += j
	}
	fmt.Println("cond-only", c, j)

	// (c) infinite for
	k := 0
	s := 0
	for {
		k++
		if k > 6 {
			break
		}
		if k%3 == 0 {
			continue
		}
		s += k
	}
	fmt.Println("infinite", s)

	// (d) continue inside a switch, inside a for
	t := 0
	for i := 0; i < 6; i++ {
		switch i % 3 {
		case 0:
			continue
		case 1:
			t += i
		default:
			t += 100
		}
		t++
	}
	fmt.Println("switch", t)

	// (e) continue inside a switch WITH fallthrough
	u := 0
	for i := 0; i < 6; i++ {
		switch i % 3 {
		case 0:
			u += 1
			fallthrough
		case 1:
			u += 10
			if i > 2 {
				continue
			}
		default:
			u += 100
		}
		u += 1000
	}
	fmt.Println("fallthrough", u)

	// (f) continue inside a select, inside a for
	ch := make(chan int, 6)
	for i := 1; i <= 6; i++ {
		ch <- i
	}
	close(ch)
	v := 0
	rounds := 0
loop:
	for {
		rounds++
		select {
		case x, ok := <-ch:
			if !ok {
				break loop
			}
			if x%2 == 0 {
				continue
			}
			v += x
		}
	}
	fmt.Println("select", v, rounds)

	// (g) continue inside range over a slice / map / string
	w := 0
	for i, e := range []int{5, 6, 7, 8} {
		if i == 1 {
			continue
		}
		w += e
	}
	fmt.Println("range-slice", w)

	x := 0
	for _, r := range "abcde" {
		if r == 'c' {
			continue
		}
		x += int(r)
	}
	fmt.Println("range-string", x)

	// (h) continue inside range over a CHANNEL (the new lowering)
	c2 := make(chan int, 6)
	for i := 1; i <= 6; i++ {
		c2 <- i
	}
	close(c2)
	y := 0
	iters := 0
	for e := range c2 {
		iters++
		if e%2 == 0 {
			continue
		}
		y += e
	}
	fmt.Println("range-chan", y, iters)

	// (i) labelled continue targeting an outer three-clause for from inside a
	// range-over-channel: the outer post clause must still run.
	c3 := make(chan int, 2)
	c3 <- 1
	c3 <- 2
	close(c3)
	outerSteps := 0
Outer:
	for a := 0; a < 3; a++ {
		outerSteps++
		for e := range c3 {
			if e == 1 {
				continue Outer
			}
		}
	}
	fmt.Println("labelled-outer", outerSteps)
}
