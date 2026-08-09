// `recover()`'s frame rules. A deferred function runs *normally* even while a
// panic is propagating — it may call other functions — and only a `recover()`
// that deferred function makes itself stops the panic. A `recover()` one call
// deeper returns nil.
package main

import (
	"fmt"
	"sync"
)

func helper() { fmt.Println("helper ran") }

func lifo() {
	defer fmt.Println("d1")
	defer fmt.Println("d2")
	defer fmt.Println("d3")
	fmt.Println("body")
}

// A deferred closure that mutates a named result.
func named() (r int) {
	defer func() { r *= 2 }()
	r = 21
	return r
}

// The same, on the panic path.
func namedPanic() (r string) {
	defer func() {
		if rec := recover(); rec != nil {
			r = fmt.Sprint("recovered:", rec)
		}
	}()
	panic("boom")
}

func loopDefer() {
	for i := 0; i < 3; i++ {
		defer fmt.Println("loop", i)
	}
}

// The deferred function calls something else *before* recovering. The panic
// must survive that call for the `recover()` to still see it.
func indirect() (ok bool) {
	defer func() {
		helper()
		ok = recover() != nil
	}()
	panic("x")
}

// An inner panic is recovered by an inner deferred function; the outer panic
// that follows is recovered by the outer one.
func nestedPanic() {
	defer func() { fmt.Println("outer rec:", recover()) }()
	func() {
		defer func() { fmt.Println("inner rec:", recover()) }()
		panic("inner")
	}()
	panic("outer")
}

// Deferred callees of every shape: a named function, a func-valued variable,
// a method value, and a literal taking an argument.
type t struct{ n int }

func (x t) m()  { fmt.Println("method rec:", recover()) }
func namedRec() { fmt.Println("named rec:", recover()) }

func viaNamed() { defer namedRec(); panic("A") }
func viaVar() {
	fv := func() { fmt.Println("fnvar rec:", recover()) }
	defer fv()
	panic("B")
}
func viaMethod() { defer t{1}.m(); panic("C") }
func viaArg() {
	defer func(k int) { fmt.Println("arg rec:", recover(), k) }(9)
	panic("D")
}

func main() {
	lifo()
	fmt.Println(named())
	fmt.Println(namedPanic())
	loopDefer()
	fmt.Println(indirect())
	nestedPanic()

	// `recover()` from a function the deferred function called is NOT the
	// direct one, so it returns nil and the direct call still gets the panic.
	func() {
		defer func() {
			f := func() any { return recover() }
			fmt.Println("nested-call recover:", f())
			fmt.Println("direct:", recover())
		}()
		panic("p2")
	}()

	// `recover()` outside a deferred function is always nil.
	fmt.Println("bare recover:", recover())

	viaNamed()
	viaVar()
	viaMethod()
	viaArg()

	// A defer runs on every exit path, including an early return.
	fmt.Println(func() int {
		defer fmt.Println("de")
		if true {
			return 7
		}
		return 0
	}())

	// Each goroutine recovers its own panic. The parked panic is per deferred
	// call, so one goroutine's must not be visible to another's `recover()`.
	var wg sync.WaitGroup
	res := make(chan string, 4)
	for i := 0; i < 3; i++ {
		wg.Add(1)
		go func(k int) {
			defer wg.Done()
			defer func() {
				if r := recover(); r != nil {
					res <- fmt.Sprint("rec", k, r)
				}
			}()
			if k%2 == 0 {
				panic(k * 10)
			}
			res <- fmt.Sprint("ok", k)
		}(i)
	}
	wg.Wait()
	close(res)
	got := []string{}
	for s := range res {
		got = append(got, s)
	}
	// Sorted, so goroutine completion order does not affect the output.
	for i := 0; i < len(got); i++ {
		for j := i + 1; j < len(got); j++ {
			if got[j] < got[i] {
				got[i], got[j] = got[j], got[i]
			}
		}
	}
	fmt.Println(got)

	// A deferred function that yields to the scheduler (a channel send) before
	// it recovers still recovers.
	sink := make(chan int, 1)
	func() {
		defer func() {
			sink <- 1
			fmt.Println("after send, rec:", recover())
		}()
		panic("q")
	}()
	fmt.Println(<-sink)
}
