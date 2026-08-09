// Receiving with `ok`: `v, ok := <-ch`, `for v := range ch`, and the comma-ok
// `select` case. `ok` is false exactly when the channel is closed AND drained,
// in which case `v` is the element type's zero — which a channel carrying a
// real zero must not be confused with.
package main

import (
	"fmt"
	"sync"
)

type pt struct{ x, y int }

func main() {
	ch := make(chan int, 3)
	ch <- 1
	ch <- 2
	ch <- 3
	close(ch)
	for v := range ch {
		fmt.Println(v)
	}

	// A goroutine producer closing the channel ends the range.
	sc := make(chan string)
	go func() {
		for _, w := range []string{"a", "bb", "ccc"} {
			sc <- w
		}
		close(sc)
	}()
	total := 0
	for s := range sc {
		total += len(s)
	}
	fmt.Println("total", total)

	// break / continue inside a channel range, unlabelled and labelled.
	c2 := make(chan int, 6)
	for i := 1; i <= 6; i++ {
		c2 <- i
	}
	close(c2)
	sum := 0
	for v := range c2 {
		if v%2 == 0 {
			continue
		}
		if v > 4 {
			break
		}
		sum += v
	}
	fmt.Println("sum", sum)

	c3 := make(chan int, 4)
	for i := 1; i <= 4; i++ {
		c3 <- i
	}
	close(c3)
outer:
	for v := range c3 {
		for j := 0; j < 3; j++ {
			if v == 3 {
				break outer
			}
		}
		fmt.Println("v", v)
	}

	// A channel carrying real zeros still terminates only at close+drain.
	z := make(chan int, 3)
	z <- 0
	z <- 0
	z <- 5
	close(z)
	cnt := 0
	for v := range z {
		cnt++
		fmt.Println("z", v)
	}
	fmt.Println("cnt", cnt)

	// Workers ranging a shared job channel.
	jobs := make(chan int, 10)
	results := make(chan int, 10)
	var wg sync.WaitGroup
	for w := 0; w < 3; w++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for j := range jobs {
				results <- j * j
			}
		}()
	}
	for i := 1; i <= 5; i++ {
		jobs <- i
	}
	close(jobs)
	wg.Wait()
	close(results)
	acc := 0
	for r := range results {
		acc += r
	}
	fmt.Println("acc", acc)

	// comma-ok over element types whose zero is not an integer zero.
	pc := make(chan pt, 1)
	pc <- pt{1, 2}
	close(pc)
	p1, ok1 := <-pc
	p2, ok2 := <-pc
	fmt.Println(p1, ok1, p2, ok2)

	bc := make(chan bool, 1)
	bc <- false
	close(bc)
	b1, okb1 := <-bc
	b2, okb2 := <-bc
	fmt.Println(b1, okb1, b2, okb2)

	strc := make(chan string, 1)
	strc <- ""
	close(strc)
	s1, oks1 := <-strc
	s2, oks2 := <-strc
	fmt.Printf("%q %v %q %v\n", s1, oks1, s2, oks2)

	// A closed channel makes its select case ready; comma-ok reports it.
	d := make(chan int, 3)
	d <- 7
	d <- 8
	close(d)
	dsum := 0
loop:
	for {
		select {
		case v, ok := <-d:
			if !ok {
				break loop
			}
			dsum += v
		}
	}
	fmt.Println("dsum", dsum)

	// select with default, empty then ready.
	e := make(chan int, 1)
	select {
	case x := <-e:
		fmt.Println("got", x)
	default:
		fmt.Println("empty")
	}
	e <- 9
	select {
	case x := <-e:
		fmt.Println("got", x)
	default:
		fmt.Println("empty")
	}

	// The blank identifier in a comma-ok select case.
	f := make(chan int)
	close(f)
	select {
	case _, ok := <-f:
		fmt.Println("closed ok:", ok)
	default:
		fmt.Println("default")
	}

	// A select send that would block falls to default.
	g := make(chan int, 1)
	g <- 1
	select {
	case g <- 2:
		fmt.Println("sent")
	default:
		fmt.Println("full")
	}

	// comma-ok into *existing* variables (plain `=`, not `:=`).
	h := make(chan int, 1)
	h <- 5
	close(h)
	var hv int
	var hok bool
	hv, hok = <-h
	fmt.Println(hv, hok)
	hv, hok = <-h
	fmt.Println(hv, hok)

	// The blank identifier on either side of a comma-ok receive.
	i := make(chan int, 1)
	i <- 3
	close(i)
	_, iok := <-i
	iv, _ := <-i
	fmt.Println(iok, iv)

	// A bare receive statement, and a receive as a sub-expression.
	j := make(chan int)
	close(j)
	<-j
	fmt.Println("drained")
	k := make(chan int, 2)
	k <- 4
	close(k)
	fmt.Println(<-k + 1)
	fmt.Println(<-k)

	// Element types whose zero is a typed nil: a closed receive yields the
	// nil slice / nil map, not an untyped nil.
	l := make(chan []int, 2)
	l <- []int{1, 2}
	l <- []int{3}
	close(l)
	for s := range l {
		fmt.Println(s, len(s))
	}
	m := make(chan map[string]int, 1)
	close(m)
	mv, mok := <-m
	fmt.Println(mv, mok, mv == nil, len(mv))
}
