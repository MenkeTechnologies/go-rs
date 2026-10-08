// A conversion to an inline interface type, `interface{}(x)` and
// `interface{ M() T }(x)`, in expression position.
package main

import "fmt"

type W int

func (w W) String() string { return "W!" }

func main() {
	fmt.Println(interface{}(5), interface{}("a") == "a")
	fmt.Println(interface{ String() string }(W(4)))
	fmt.Printf("%T %T\n", interface{}(W(2)), interface{ String() string }(W(3)))
	xs := []interface{}{interface{}(1), interface{}(2.5)}
	fmt.Println(xs, len(xs))
	var s fmt.Stringer = interface{ String() string }(W(7))
	fmt.Println(s.String())
}
