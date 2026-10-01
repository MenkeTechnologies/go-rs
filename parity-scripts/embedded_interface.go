package main

import "fmt"

// A struct embedding an interface gets the interface's methods promoted onto
// it; a method the struct declares itself overrides the promoted one.

type Lener interface {
	Len() int
	Less(i, j int) bool
	Name() string
}

type ints []int

func (x ints) Len() int           { return len(x) }
func (x ints) Less(i, j int) bool { return x[i] < x[j] }
func (x ints) Name() string       { return "ints" }

type rev struct {
	Lener
}

func (r rev) Less(i, j int) bool { return r.Lener.Less(j, i) }

type tagged struct {
	Lener
	tag string
}

func (t *tagged) Name() string { return t.tag + ":" + t.Lener.Name() }

func wrap(l Lener) Lener { return &rev{l} }

func main() {
	var l Lener = ints{1, 2, 3}
	r := rev{l}
	fmt.Println(r.Len(), r.Less(0, 1), r.Lener.Len(), r.Name())
	w := wrap(l)
	fmt.Println(w.Len(), w.Less(0, 1), w.Name())
	var t Lener = &tagged{w, "t"}
	fmt.Println(t.Len(), t.Less(1, 2), t.Name())
}
