// defer / panic / recover: LIFO deferred calls with arguments snapshotted at
// defer time, a deferred pointer-receiver method observing later mutations, a
// panic unwinding across a call frame and caught by recover() in a deferred
// closure, and the recovered function returning its result's zero values.
package main

import "fmt"

type Counter struct {
	n int
}

func (c *Counter) Inc() {
	c.n++
}

func (c *Counter) Report() {
	fmt.Println("count:", c.n)
}

func lifo() {
	for i := 0; i < 4; i++ {
		defer fmt.Println("deferred", i)
	}
	fmt.Println("body of lifo")
}

func withMethod() {
	var c Counter
	defer c.Report()
	c.Inc()
	c.Inc()
	c.Inc()
	fmt.Println("incremented three times")
}

func mightPanic(n int) int {
	if n > 5 {
		panic("value too large")
	}
	return n * 2
}

func guarded(n int) (out int) {
	defer func() {
		if r := recover(); r != nil {
			fmt.Println("recovered from:", r)
		}
	}()
	out = mightPanic(n)
	fmt.Println("no panic for", n)
	return out
}

func main() {
	lifo()
	fmt.Println("---")
	withMethod()
	fmt.Println("---")
	fmt.Println("guarded(3) =", guarded(3))
	fmt.Println("guarded(9) =", guarded(9))
	fmt.Println("done")
}
