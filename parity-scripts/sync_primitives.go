// sync.WaitGroup, Mutex, RWMutex and Once driving real goroutines. Every
// printed value is order-independent (sums, counts, sorted output), because Go
// does not specify goroutine scheduling.
package main

import (
	"fmt"
	"sort"
	"sync"
)

type counter struct {
	mu sync.Mutex
	n  int
}

func (c *counter) inc() {
	c.mu.Lock()
	c.n++
	c.mu.Unlock()
}

func main() {
	// The canonical fan-out: Add before the go, Done via defer, Wait after.
	var wg sync.WaitGroup
	var mu sync.Mutex
	total := 0
	for i := 1; i <= 10; i++ {
		wg.Add(1)
		go func(n int) {
			defer wg.Done()
			mu.Lock()
			total += n * n
			mu.Unlock()
		}(i)
	}
	wg.Wait()
	fmt.Println("sum of squares:", total)

	// A mutex guarding a struct field, hammered from many goroutines.
	c := &counter{}
	var wg2 sync.WaitGroup
	for i := 0; i < 50; i++ {
		wg2.Add(1)
		go func() {
			defer wg2.Done()
			c.inc()
		}()
	}
	wg2.Wait()
	fmt.Println("count:", c.n)

	// Goroutines that both use a channel and a WaitGroup: the channel is what
	// makes them interleave at all, so this exercises Wait across real parks.
	results := make(chan int, 8)
	var wg3 sync.WaitGroup
	for i := 1; i <= 8; i++ {
		wg3.Add(1)
		go func(n int) {
			defer wg3.Done()
			results <- n * 3
		}(i)
	}
	wg3.Wait()
	close(results)
	got := []int{}
	for i := 0; i < 8; i++ {
		got = append(got, <-results)
	}
	sort.Ints(got)
	fmt.Println("results:", got)

	// Wait with nothing outstanding returns immediately; a second Wait too.
	var idle sync.WaitGroup
	idle.Wait()
	idle.Add(1)
	idle.Done()
	idle.Wait()
	fmt.Println("idle ok")

	// Once runs its function exactly once, even across goroutines.
	var once sync.Once
	runs := 0
	var wg4 sync.WaitGroup
	for i := 0; i < 5; i++ {
		wg4.Add(1)
		go func() {
			defer wg4.Done()
			once.Do(func() { runs++ })
		}()
	}
	wg4.Wait()
	fmt.Println("once runs:", runs)

	// TryLock reports whether it took the lock.
	var m sync.Mutex
	fmt.Println("trylock free:", m.TryLock())
	fmt.Println("trylock held:", m.TryLock())
	m.Unlock()
	fmt.Println("trylock again:", m.TryLock())
	m.Unlock()

	// RWMutex: many readers, then an exclusive writer.
	var rw sync.RWMutex
	shared := 0
	var wg5 sync.WaitGroup
	for i := 0; i < 4; i++ {
		wg5.Add(1)
		go func() {
			defer wg5.Done()
			rw.RLock()
			_ = shared
			rw.RUnlock()
		}()
	}
	wg5.Wait()
	rw.Lock()
	shared = 42
	rw.Unlock()
	rw.RLock()
	fmt.Println("shared:", shared)
	rw.RUnlock()
}
