// Naming an interface type in call position is Go's identity conversion:
// `error(x)`, `any(x)`, and any declared interface type.
package main

import (
	"errors"
	"fmt"
)

type myErr struct{ s string }

func (e myErr) Error() string { return e.s }

type Stringer interface{ String() string }

type Shape interface {
	Area() float64
	Name() string
}

type sq struct{ side float64 }

func (s sq) Area() float64 { return s.side * s.side }
func (s sq) Name() string  { return "square" }
func (s sq) String() string {
	return fmt.Sprintf("sq(%v)", s.side)
}

func describe(v any) string { return fmt.Sprintf("%v/%T", v, v) }

func main() {
	e := error(myErr{"boom"})
	fmt.Println(e, e.Error())

	fmt.Println(any(3), any("s"), any(true), any(2.5))
	fmt.Printf("%v %T | %v %T\n", any(7), any(7), any("x"), any("x"))

	s := Stringer(sq{4})
	fmt.Println(s.String())

	var sh = Shape(sq{3})
	fmt.Println(sh.Name(), sh.Area())

	// The conversion is the identity, so pointer identity survives it.
	base := errors.New("base")
	fmt.Println(error(base) == base, errors.Is(error(base), base))

	// It composes with the ordinary conversions and with `describe`'s `any`.
	fmt.Println(describe(any(1)), describe(any("x")))
	fmt.Println(any(float64(1)) == any(float64(1)))
}
