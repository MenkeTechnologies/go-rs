// `sync/atomic` functions and types over a captured counter, with goroutines.
package main

import (
	"fmt"
	"sync"
	"sync/atomic"
)

func main() {
	var n int64
	atomic.AddInt64(&n, 5)
	fmt.Println(atomic.LoadInt64(&n), n)
	var c atomic.Int32
	c.Add(3)
	fmt.Println(c.Load(), c.CompareAndSwap(3, 9), c.Load())
	var wg sync.WaitGroup
	for i := 0; i < 5; i++ {
		wg.Add(1)
		go func(d int) {
			defer wg.Done()
			atomic.AddInt64(&n, int64(d))
		}(i)
	}
	wg.Wait()
	fmt.Println(n)
}
