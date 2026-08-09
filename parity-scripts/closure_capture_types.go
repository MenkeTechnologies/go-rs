// A captured variable keeps its declared type inside the closure. A closure
// body is compiled with a fresh symbol table, so the width a variable was
// declared with has to be carried in explicitly — otherwise `uint8` arithmetic
// stops wrapping, `float32` stops rounding to 32 bits, `uint64` starts reading
// signed, and a captured channel stops being a channel. Every one of those is a
// silently wrong value, not an error.
package main

import "fmt"

type pt struct{ x, y int }

func main() {
	// float32: 32-bit arithmetic and the 32-bit shortest decimal.
	var f float32 = 1.0 / 3.0
	g := func() { fmt.Println(f, f*3, f+f) }
	g()
	fmt.Println(f, f*3, f+f)

	// Narrow integers wrap at their declared width inside the closure.
	var b uint8 = 250
	h := func() uint8 { b += 10; return b }
	fmt.Println(h(), b)

	var i8 int8 = 120
	k := func() { i8 += 20; fmt.Println(i8, i8>>2, i8/3) }
	k()

	var u16 uint16 = 65530
	l := func() { u16 += 10; fmt.Println(u16, u16>>3) }
	l()

	var i32 int32 = 2147483640
	n := func() { i32 += 10; fmt.Println(i32, i32>>4) }
	n()

	// Container element types survive too.
	xs := []float32{1.0 / 3.0, 2.0 / 3.0}
	m := map[string]uint64{"a": 1 << 63}
	p := pt{1, 2}
	q := func() { fmt.Println(xs, m, p, p.x+p.y) }
	q()

	// Two levels of nesting.
	var u uint64 = 1 << 63
	outer := func() {
		inner := func() { fmt.Println(u, u/2, u > 1) }
		inner()
	}
	outer()

	// A captured value mutated by the closure keeps its type afterwards.
	var c uint64 = 5
	dec := func() { c -= 10 }
	dec()
	fmt.Println(c, c/2)

	s := "abc"
	t := func() { fmt.Println(s+"d", len(s)) }
	t()

	// The same widths through a goroutine body and a deferred closure, which
	// are closures as well.
	done := make(chan uint8, 1)
	var gb uint8 = 200
	go func() { gb *= 2; done <- gb }()
	fmt.Println(<-done)

	func() {
		var db int8 = 100
		defer func() { db += 100; fmt.Println("deferred", db) }()
	}()
}
