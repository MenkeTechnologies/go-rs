// A defined type passed to `panic` or to an `...error` / `...any` parameter is
// an interface value carrying its type (and so its Error method); a chain of
// pointer-receiver calls writes through to the variable; `(*T)(x)` is a
// conversion.
package main

import (
	"errors"
	"fmt"
)

type Code string

func (c Code) Error() string { return "code " + string(c) }

type Level int

func (l Level) String() string { return fmt.Sprintf("L%d", int(l)) }

type Counter struct{ n int }

func (c Counter) Value() int { return c.n }
func (c *Counter) Add(d int) *Counter {
	c.n += d
	return c
}

func show(vs ...any) {
	for _, v := range vs {
		fmt.Print(v, "|")
	}
	fmt.Println()
}

func main() {
	func() {
		defer func() {
			r := recover()
			err, ok := r.(error)
			fmt.Println(r, ok, err)
		}()
		panic(Code("x"))
	}()
	show(Level(3), Code("y"), 4, "s")
	j := errors.Join(Code("a"), nil, Code("b"))
	var c Code
	fmt.Println(j, errors.As(j, &c), c)
	fmt.Println(errors.Is(j, Code("b")), errors.Is(j, Code("z")))
	var ctr Counter
	fmt.Println(ctr.Add(2).Add(3).Value(), ctr.n)
	val := ctr.Value
	pf := (*Counter).Add
	pf(&ctr, 10)
	fmt.Println(val(), ctr.Value(), ctr.Add(1).Add(1).n)
	var p = (*Counter)(nil)
	fmt.Println(p == nil)
}
