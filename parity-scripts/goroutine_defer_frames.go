// Deferred calls belong to the goroutine that deferred them. Two pipeline
// stages that each `defer close(out)` interleave on one thread; each stage
// must close its own channel when it returns, not the other stage's.
package main

import (
	"fmt"
	"sync"
)

func gen(n int) <-chan int {
	ch := make(chan int)
	go func() {
		defer close(ch)
		for i := 0; i < n; i++ {
			ch <- i * i
		}
	}()
	return ch
}

func inc(in <-chan int) <-chan int {
	out := make(chan int)
	go func() {
		defer close(out)
		for v := range in {
			out <- v + 1
		}
	}()
	return out
}

// depth recurses n levels; each level's deferred call appends to the trail it
// returns, so the trail lists the levels in the order their defers ran.
func depth(n int) (d int, trail string) {
	defer func() { trail += fmt.Sprint(" leave", n) }()
	if n == 0 {
		return 0, ""
	}
	d, trail = depth(n - 1)
	return d + 1, trail
}

func main() {
	for v := range inc(inc(gen(5))) {
		fmt.Print(v, " ")
	}
	fmt.Println()

	var wg sync.WaitGroup
	var mu sync.Mutex
	results := make([]string, 3)
	for w := 0; w < 3; w++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			d, trail := depth(w + 1)
			mu.Lock()
			defer mu.Unlock()
			results[w] = fmt.Sprint(d, trail)
		}()
	}
	wg.Wait()
	for _, r := range results {
		fmt.Println(r)
	}

	done := make(chan string)
	for i := 0; i < 2; i++ {
		go func() {
			defer func() {
				if r := recover(); r != nil {
					done <- fmt.Sprint("recovered ", r)
				}
			}()
			ch := make(chan int)
			defer close(ch)
			if i == 1 {
				panic(fmt.Sprint("worker ", i))
			}
			done <- fmt.Sprint("worker ", i, " ok")
		}()
	}
	got := []string{<-done, <-done}
	if got[0] > got[1] {
		got[0], got[1] = got[1], got[0]
	}
	fmt.Println(got)
}
