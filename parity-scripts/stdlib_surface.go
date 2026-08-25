package main

// A sweep of the surface a real Go program leans on, in one place: `errors`
// wrap chains through `Is`/`As`/`Unwrap`, an interface embedding two others,
// method values and expressions, `sort.SliceStable`'s stability, the `sync`
// primitives under real goroutines, `defer` with a loop variable under Go
// 1.22 per-iteration semantics, and the `strconv` conversions in both
// directions including the error path.

import (
	"errors"
	"fmt"
	"sort"
	"strconv"
	"sync"
)

type myErr struct{ code int }

func (e *myErr) Error() string { return "code " + strconv.Itoa(e.code) }

type Shape interface{ Area() int }
type Named interface{ Name() string }
type Both interface {
	Shape
	Named
}

type sq struct{ s int }

func (s sq) Area() int    { return s.s * s.s }
func (s sq) Name() string { return "sq" }

type counter struct {
	mu sync.Mutex
	n  int
}

func (c *counter) inc() { c.mu.Lock(); c.n++; c.mu.Unlock() }

func main() {
	// errors.Is / As / Unwrap chains
	base := &myErr{7}
	w1 := fmt.Errorf("layer1: %w", base)
	w2 := fmt.Errorf("layer2: %w", w1)
	var target *myErr
	fmt.Println("errors", w2, errors.Is(w2, base), errors.As(w2, &target), target.code)
	fmt.Println("unwrap", errors.Unwrap(w2) == w1, errors.Unwrap(errors.Unwrap(w2)) == base)

	// embedded interface satisfaction
	var b Both = sq{3}
	var s Shape = b
	fmt.Println("embedded-iface", b.Area(), b.Name(), s.Area())

	// method values vs method expressions
	q := sq{4}
	mv := q.Area
	me := sq.Area
	fmt.Println("method-vals", mv(), me(q), me(sq{5}))

	// sort.Slice stability
	type kv struct {
		K string
		V int
	}
	xs := []kv{{"b", 1}, {"a", 2}, {"b", 0}, {"a", 1}}
	sort.SliceStable(xs, func(i, j int) bool { return xs[i].K < xs[j].K })
	fmt.Println("stable", xs)

	// sync primitives
	var wg sync.WaitGroup
	c := &counter{}
	for i := 0; i < 50; i++ {
		wg.Add(1)
		go func() { defer wg.Done(); c.inc() }()
	}
	wg.Wait()
	var once sync.Once
	hits := 0
	for i := 0; i < 3; i++ {
		once.Do(func() { hits++ })
	}
	fmt.Println("sync", c.n, hits)

	// defer with loop variables (Go 1.22 per-iteration semantics)
	func() {
		for i := 0; i < 3; i++ {
			defer fmt.Print("d", i, " ")
		}
	}()
	fmt.Println()

	// strconv surface
	i1, e1 := strconv.Atoi("42")
	_, e2 := strconv.Atoi("4x")
	fmt.Println("strconv", i1, e1, e2 != nil, strconv.Itoa(-7), strconv.FormatInt(255, 16), strconv.Quote("a\tb"))
	b1, _ := strconv.ParseBool("true")
	f1, _ := strconv.ParseFloat("3.5", 64)
	fmt.Println("strconv2", b1, f1, strconv.FormatBool(false), strconv.FormatFloat(1.5, 'f', 2, 64))
}
