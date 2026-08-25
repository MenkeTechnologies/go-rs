package main

// The `select` and channel shapes that work, pinned so they keep working.
//
// Two that do not are recorded in BUGS.md rather than here, because this file
// is a byte-parity gate: a `select` whose *send* case blocks until a receiver
// arrives, and a nil channel in a `select`.

import (
	"fmt"
	"sync"
)

func main() {
	// A default with nothing ready, and with exactly one case ready.
	c := make(chan int, 1)
	select {
	case v := <-c:
		fmt.Println("got", v)
	default:
		fmt.Println("empty")
	}
	c <- 5
	select {
	case v := <-c:
		fmt.Println("one-ready", v)
	default:
		fmt.Println("no")
	}

	// Two channels, one ever ready — the choice is deterministic.
	a, b := make(chan string, 1), make(chan string, 1)
	a <- "A"
	select {
	case v := <-a:
		fmt.Println("pick", v)
	case v := <-b:
		fmt.Println("pick", v)
	}

	// A receive case fed by a sender that parked first.
	u := make(chan int)
	go func() { u <- 42 }()
	select {
	case v := <-u:
		fmt.Println("parked-sender", v)
	}

	// The same with an alternative case that is not ready.
	u2 := make(chan int)
	idle := make(chan struct{})
	go func() { u2 <- 7 }()
	select {
	case <-idle:
		fmt.Println("idle")
	case v := <-u2:
		fmt.Println("two-case", v)
	}

	// A send case with room, and one without.
	room := make(chan int, 2)
	select {
	case room <- 1:
		fmt.Println("sent", <-room)
	default:
		fmt.Println("blocked")
	}
	full := make(chan int, 1)
	full <- 1
	select {
	case full <- 2:
		fmt.Println("sent")
	default:
		fmt.Println("would-block")
	}

	// A closed channel is always ready.
	done := make(chan struct{})
	close(done)
	select {
	case <-done:
		fmt.Println("closed-ready")
	default:
		fmt.Println("no")
	}

	// Draining a closed buffered channel with range.
	src := make(chan int, 4)
	for i := 1; i <= 4; i++ {
		src <- i
	}
	close(src)
	total := 0
	for v := range src {
		total += v
	}
	fmt.Println("drain", total)

	// A worker pool over a closed job queue, with a WaitGroup.
	jobs := make(chan int, 8)
	res := make(chan int, 8)
	for i := 1; i <= 8; i++ {
		jobs <- i
	}
	close(jobs)
	var wg sync.WaitGroup
	for w := 0; w < 3; w++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for j := range jobs {
				res <- j * j
			}
		}()
	}
	wg.Wait()
	close(res)
	sum := 0
	for v := range res {
		sum += v
	}
	fmt.Println("pool", sum)

	// comma-ok receive distinguishes a zero from a closed channel.
	z := make(chan int, 1)
	z <- 0
	v1, ok1 := <-z
	close(z)
	v2, ok2 := <-z
	fmt.Println("comma-ok", v1, ok1, v2, ok2)
}
