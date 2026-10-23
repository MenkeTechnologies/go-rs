// Type aliases: `type A = B` names B itself — the same identity, method set
// and `%T` — for a struct, a basic type, a composite, another package's type,
// a defined type with methods, an alias of an alias declared later, a
// block-local alias, and an import used only by an alias that is never used.
package main

import (
	"fmt"
	"io"
	"strings"
)

type Point struct{ X, Y int }

func (p Point) String() string { return fmt.Sprintf("(%d,%d)", p.X, p.Y) }

type P = Point

type (
	Celsius = float64
	Names   = []string
	Table   = map[string]int
	Builder = strings.Builder
)

type Unused = io.Reader

type Temp float64

func (t Temp) Hot() bool { return t > 30 }

type Reading = Temp

type Outer = Inner
type Inner = [3]int

func half(c Celsius) Celsius { return c / 2 }

func main() {
	p := P{1, 2}
	var q Point = p
	fmt.Println(p, q, p == q)
	fmt.Printf("%T %T %v\n", p, &p, P{Y: 9})

	var c Celsius = 3
	fmt.Printf("%v %T\n", half(c), c)

	ns := Names{"b", "a"}
	ns = append(ns, "c")
	fmt.Println(ns, len(ns))
	tb := Table{"x": 1}
	tb["y"] = 2
	fmt.Println(tb, len(tb))

	var sb Builder
	sb.WriteString("built")
	fmt.Println(sb.String(), sb.Len())

	var r Reading = 40
	fmt.Println(r.Hot(), Reading(10).Hot())
	fmt.Printf("%T\n", r)

	o := Outer{1, 2}
	fmt.Printf("%v %T %d\n", o, o, len(o))

	type local = int
	var l local = 5
	fmt.Printf("%T %d\n", l, l+1)

	var v any = P{3, 4}
	if pt, ok := v.(Point); ok {
		fmt.Println("point", pt.X)
	}
	switch v.(type) {
	case P:
		fmt.Println("case P")
	}
}
