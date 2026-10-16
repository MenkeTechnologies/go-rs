package main

import (
	"errors"
	"fmt"
)

type A struct{ n int }

func (a A) String() string { return "A" }

type C float64

func (c C) String() string { return "C" }

type S string
type SS string

func (s SS) String() string { return "ss" }

type E struct{}

func (E) Error() string { return "E" }

type P struct{ x int }

func (p *P) String() string { return "P" }

func main() {
	e := errors.New("e")
	fmt.Println(fmt.Sprint(A{}, A{}) + "|")
	fmt.Println(fmt.Sprint(C(1), C(2)) + "|")
	fmt.Println(fmt.Sprint(S("a"), S("b")) + "|")
	fmt.Println(fmt.Sprint(SS("a"), SS("b")) + "|")
	fmt.Println(fmt.Sprint(e, e) + "|")
	fmt.Println(fmt.Sprint(E{}, E{}) + "|")
	fmt.Println(fmt.Sprint(&P{}, &P{}) + "|")
	fmt.Println(fmt.Sprint(1, A{}, "x", A{}, 2) + "|")
	fmt.Print(A{}, A{}, "\n")
	fmt.Print(e, e, "\n")
	fmt.Print(S("a"), S("b"), 1, 2, S("c"), "\n")
	fmt.Println(fmt.Sprint(S("p"), 1, 2, S("q")))
	var x fmt.Stringer = A{}
	fmt.Println(fmt.Sprint(x, x) + "|")
}
