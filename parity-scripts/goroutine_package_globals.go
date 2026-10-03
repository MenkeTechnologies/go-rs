package main

import (
	"fmt"
	"sync"
)

var counter = 5
var log []string
var totals = map[string]int{}
var mu sync.Mutex

type stats struct{ hits, misses int }

var st stats

func work(done chan bool) {
	counter = 100
	done <- true
}

func reader(out chan int) { out <- counter * 2 }

func appendLog(id int, wg *sync.WaitGroup) {
	defer wg.Done()
	mu.Lock()
	log = append(log, fmt.Sprintf("w%d", id))
	totals["n"] += id
	st.hits++
	mu.Unlock()
}

func main() {
	done := make(chan bool)
	counter = 7
	go work(done)
	<-done
	fmt.Println(counter)

	out := make(chan int)
	counter = 21
	go reader(out)
	fmt.Println(<-out)

	var wg sync.WaitGroup
	for i := 1; i <= 4; i++ {
		wg.Add(1)
		go appendLog(i, &wg)
	}
	wg.Wait()
	fmt.Println(len(log), totals["n"], st.hits, st)

	res := make(chan int, 3)
	for i := 0; i < 3; i++ {
		go func(k int) {
			counter += k
			res <- k
		}(i)
	}
	sum := 0
	for i := 0; i < 3; i++ {
		sum += <-res
	}
	fmt.Println(sum, counter)
}
