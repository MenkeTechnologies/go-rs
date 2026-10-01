// A closure that writes a package-level variable writes the one variable
// every function sees — from main, from another function, and from a
// goroutine — rather than a copy or a cell another function would print.
package main

import (
	"fmt"
	"sync"
)

var (
	counter int
	log     []string
	names   = map[string]int{}
)

func peek() int { return counter }

func record(s string) { log = append(log, s) }

func bump() {
	f := func() { counter++; record(fmt.Sprint("bump ", peek())) }
	f()
	f()
}

func main() {
	bump()
	fmt.Println(counter, peek(), log)
	g := func(n int) { counter += n; names["g"] = counter }
	g(10)
	fmt.Println(counter, peek(), names)
	var wg sync.WaitGroup
	var mu sync.Mutex
	for i := 0; i < 5; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			mu.Lock()
			counter++
			mu.Unlock()
		}()
	}
	wg.Wait()
	fmt.Println(counter, peek(), len(log))
	local := 0
	inc := func() { local++ }
	inc()
	inc()
	fmt.Println(local)
	fs := []func() int{}
	for i := 0; i < 3; i++ {
		fs = append(fs, func() int { return i * 10 })
	}
	for _, f := range fs {
		fmt.Print(f(), " ")
	}
	fmt.Println()
}
