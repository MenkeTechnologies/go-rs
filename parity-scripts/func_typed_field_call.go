package main

import "fmt"

// A func-typed struct *field* is called like the function value it holds —
// `p.stage(8)`, not a method named `stage`. Go tells the two apart by the
// name's declaration, and a type cannot declare both.

type pipe struct {
	stage func(int) int
	name  string
}

func dbl(n int) int { return n * 2 }

func (p pipe) run(v int) int { return p.stage(v) }

func main() {
	p := pipe{stage: dbl, name: "double"}
	fmt.Println(p.name, p.stage(8))
	fmt.Println(p.run(21))

	// Through a pointer, and reassigned.
	q := &pipe{stage: func(n int) int { return n + 100 }}
	fmt.Println(q.stage(1))
	q.stage = dbl
	fmt.Println(q.stage(1))

	// A nested struct's func field.
	outer := struct{ inner pipe }{inner: pipe{stage: dbl}}
	fmt.Println(outer.inner.stage(3))
}
