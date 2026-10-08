package main

import "fmt"

type A struct{}

func (A) String() string { return "a" }
func (A) Name() string   { return "A" }

type B struct{}

func (B) String() string { return "b" }

type O struct {
	A
	B
}

type Deep struct{ B }

// One String at depth 1 (via A), one at depth 2 (via Deep.B): A's wins.
type P struct {
	A
	Deep
}

// A field named Name shadows the promoted method.
type Q struct {
	A
	Name string
}

type Twice struct {
	X
	Y
}
type X struct{ A }
type Y struct{ A }

func main() {
	var o any = O{}
	_, ok := o.(fmt.Stringer)
	fmt.Println(ok, O{}.Name())
	fmt.Println(o)
	var p any = P{}
	_, ok = p.(fmt.Stringer)
	fmt.Println(ok, p)
	q := Q{Name: "field"}
	fmt.Println(q.Name, q)
	var t any = Twice{}
	_, ok = t.(fmt.Stringer)
	fmt.Println(ok, t)
	_, ok = t.(interface{ Name() string })
	fmt.Println(ok)
}
