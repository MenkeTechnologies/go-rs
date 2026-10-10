// `%#v` names the type of a nil pointer, func, channel or interface — as an
// operand and as a struct field.
package main

import "fmt"

type Node struct {
	Next *Node
	Val  int
}

type Shape interface{ Area() float64 }

type Holder struct {
	P  *int
	F  func()
	G  func(int) string
	E  error
	C  chan int
	I  interface{}
	S  Shape
	N  *Node
	M  map[string]int
	L  []int
	ok bool
}

func main() {
	fmt.Printf("%#v\n", Holder{})
	var p *int
	var n *Node
	var f func()
	var c chan string
	fmt.Printf("%#v %#v %#v %#v\n", p, n, f, c)
	fmt.Printf("%v %v %v %v\n", p, n, f == nil, c)
	fmt.Printf("%T %T %T\n", p, n, c)
	x := 5
	h := Holder{P: &x, N: &Node{Val: 2}}
	fmt.Println(*h.P, h.N.Val, h.N.Next == nil)
	fmt.Printf("%#v\n", Node{})
}
