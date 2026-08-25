package main

// Method values (`q.Area`) and method expressions (`sq.Area`). Both become a
// closure that calls the method, so dispatch is unchanged; what has to be right
// is where the receiver comes from.
//
// A method value evaluates and *copies* its receiver where the value is
// written, so a later write to the variable is not seen through it — and a
// pointer receiver is shared rather than copied, so that one still writes
// through. Those are the same two rules a bind follows, which is why the copy
// is the ordinary one rather than a special case.
//
// A method expression takes the receiver as its first parameter instead.

import "fmt"

type pt struct{ X int }

func (p pt) Get() int         { return p.X }
func (p *pt) Set(n int)       { p.X = n }
func (p pt) Add(a, b int) int { return p.X + a + b }
func (p pt) Show()            { fmt.Print("show", p.X, " ") }

type Getter interface{ Get() int }

func main() {
	// A method value binds the receiver NOW, by value.
	q := pt{4}
	mv := q.Get
	q.X = 99
	fmt.Println("bound-at-capture", mv(), q.X)

	// A pointer-receiver method value binds the pointer, so it writes through.
	r := pt{1}
	set := (&r).Set
	set(7)
	fmt.Println("ptr-recv-value", r.X)

	// Multiple parameters, and a void method.
	a := pt{10}
	add := a.Add
	fmt.Println("multi-arg", add(1, 2))
	sh := a.Show
	sh()
	fmt.Println()

	// A method expression takes the receiver as its first parameter.
	get := pt.Get
	fmt.Println("method-expr", get(pt{5}), get(q))

	// Passed as a function value.
	apply := func(f func() int) int { return f() }
	fmt.Println("as-arg", apply(a.Get))

	// Through an interface variable.
	var g Getter = a
	gv := g.Get
	fmt.Println("iface-method-value", gv())

	// Stored in a slice and a map.
	fs := []func() int{a.Get, q.Get}
	fm := map[string]func() int{"a": a.Get}
	fmt.Println("containers", fs[0](), fs[1](), fm["a"]())
}
