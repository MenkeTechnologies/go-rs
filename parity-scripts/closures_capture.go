// Capture-by-reference: a closure captures the variable (a shared cell), not a
// copy. A counter closure persists state across calls, two closures share one
// captured variable, mutation through a closure is observed in the enclosing
// scope, and Go 1.22 gives each loop iteration its own variable.
package main

import "fmt"

func makeCounter() func() int {
	count := 0
	return func() int {
		count++
		return count
	}
}

func makeAccumulator() (func(int), func() int) {
	total := 0
	add := func(n int) { total += n }
	get := func() int { return total }
	return add, get
}

func main() {
	c := makeCounter()
	d := makeCounter()
	fmt.Println(c(), c(), c(), d())

	add, get := makeAccumulator()
	add(5)
	add(10)
	add(2)
	fmt.Println(get())

	x := 1
	tenfold := func() { x = x * 10 }
	tenfold()
	tenfold()
	fmt.Println(x)

	f0 := func() int { return -1 }
	f1 := func() int { return -1 }
	f2 := func() int { return -1 }
	for i := 0; i < 3; i++ {
		f := func() int { return i * i }
		if i == 0 {
			f0 = f
		} else if i == 1 {
			f1 = f
		} else {
			f2 = f
		}
	}
	fmt.Println(f0(), f1(), f2())
}
