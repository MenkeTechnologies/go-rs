package main

import "fmt"

type T struct {
	A int    `json:"a"`
	B string `json:"b,omitempty" xml:"b"`
	C, D int `x:"y"`
	*E
	F struct {
		G int `g:"1"`
	} `f:"2"`
}
type E struct{ Z int }

func main() {
	t := T{A: 1, E: &E{5}}
	fmt.Printf("%v %+v\n", t.A, t.F)
	fmt.Println(t.Z)
}
