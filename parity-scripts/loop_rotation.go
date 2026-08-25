package main

// Every `for` is lowered rotated — the condition emitted once as an entry guard
// and once below the body as a conditional backward branch, which is the shape
// fusevm's tracing JIT needs to compile a hot loop. Rotation duplicates the
// condition's *code*, so what has to be pinned is that it does not duplicate the
// condition's *evaluation*: a top-test loop runs the test n+1 times for n
// iterations, and so must this one. A side-effecting condition is the only way a
// program can observe the difference.

import "fmt"

var calls int

func lt(a, b int) bool {
	calls++
	return a < b
}

func main() {
	// The evaluation count of a side-effecting condition, at n = 5.
	calls = 0
	n := 0
	for i := 0; lt(i, 5); i++ {
		n += i
	}
	fmt.Println("three-clause", n, calls)

	// The same, entered zero times: the guard runs once and nothing else does.
	calls = 0
	n = 0
	for i := 0; lt(i, 0); i++ {
		n += i
	}
	fmt.Println("zero-iteration", n, calls)

	// Condition-only (Go's `while`), where the body is what moves the counter.
	calls = 0
	j, t := 0, 0
	for lt(j, 4) {
		j++
		t += j
	}
	fmt.Println("cond-only", t, calls)

	// `continue` must reach the post statement, then the bottom test — not skip
	// the post and spin, and not skip the test and run an extra iteration.
	calls = 0
	c := 0
	for i := 0; lt(i, 6); i++ {
		if i%2 == 0 {
			continue
		}
		c += i
	}
	fmt.Println("continue", c, calls)

	// `break` leaves before the bottom test, so the count is one lower than the
	// unbroken loop's would be.
	calls = 0
	b := 0
	for i := 0; lt(i, 100); i++ {
		if i == 3 {
			break
		}
		b += i
	}
	fmt.Println("break", b, calls)

	// A nested loop re-runs the inner guard once per outer iteration.
	calls = 0
	nest := 0
	for i := 0; lt(i, 3); i++ {
		for k := 0; lt(k, 2); k++ {
			nest += i * k
		}
	}
	fmt.Println("nested", nest, calls)

	// A labeled continue jumps to the *outer* post statement and its bottom
	// test, skipping the rest of the inner loop.
	calls = 0
	lab := 0
outer:
	for i := 0; lt(i, 4); i++ {
		for k := 0; lt(k, 4); k++ {
			if k > i {
				continue outer
			}
			lab++
		}
	}
	fmt.Println("labeled-continue", lab, calls)

	// A labeled break leaves both loops from the inner one.
	calls = 0
	lb := 0
done:
	for i := 0; lt(i, 4); i++ {
		for k := 0; lt(k, 4); k++ {
			if i*k >= 2 {
				break done
			}
			lb++
		}
	}
	fmt.Println("labeled-break", lb, calls)

	// A condition whose own side effect is what ends the loop.
	sink := 0
	stop := 0
	for func() bool { stop++; return stop <= 4 }() {
		sink += stop
	}
	fmt.Println("self-terminating", sink, stop)

	// `for {}` has no condition, so it branches back on a constant `true`;
	// `break` is still the only exit.
	inf := 0
	for {
		inf++
		if inf == 7 {
			break
		}
	}
	fmt.Println("bare", inf)

	// A range loop is rotated the same way, and hoists its key count out of the
	// loop — a body that shrinks the map must not change the iteration count.
	m := map[string]int{"a": 1, "b": 2, "c": 3}
	seen := 0
	for k := range m {
		delete(m, k)
		seen++
	}
	fmt.Println("range-delete", seen, len(m))

	// Range over each container kind, with the loop entered zero times.
	empty := 0
	for range []int{} {
		empty++
	}
	for range map[int]int{} {
		empty++
	}
	for range "" {
		empty++
	}
	for range 0 {
		empty++
	}
	fmt.Println("range-empty", empty)
}
