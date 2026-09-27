package main

import "fmt"

// A Stringer enum: fmt calls String() under %v %s %q %x %X, prints the number
// under %d, and renders slice and array elements through the method too.
type Color int

const (
	Red Color = iota
	Green
	Blue
)

func (c Color) String() string { return [...]string{"R", "G", "B"}[c] }

type Level int

func (l Level) String() string {
	if l == 0 {
		return "low"
	}
	return "high"
}

type Shower interface{ String() string }

func show(s Shower) string { return s.String() }

type Pt struct{ X, Y int }

func (p Pt) String() string { return fmt.Sprintf("(%d,%d)", p.X, p.Y) }

// A pointer-receiver String is not in a value's method set.
type Ref int

func (r *Ref) String() string { return "ref" }

func main() {
	var c Color = Blue
	fmt.Println(c, Red, Level(0), Level(3))
	fmt.Println(Green.String(), show(Green), show(Level(1)))
	fmt.Println([]Color{Green, Blue}, [2]Color{Red, Blue})
	fmt.Printf("%v|%s|%d|%q|%x|%#v\n", c, c, c, c, c, c)
	fmt.Printf("%v %d\n", []Color{Red, Green}, []Color{Red, Green})
	fmt.Println(fmt.Sprintf("%5s|%-4v|", Green, Red))
	ps := []Pt{{1, 2}, {3, 4}}
	fmt.Println(ps)
	fmt.Printf("%v %s %d\n", ps, ps, Pt{5, 6})
	fmt.Println([]Ref{1, 2})
	var any1 any = Blue
	switch v := any1.(type) {
	case Color:
		fmt.Println("color", int(v)+10, v)
	}
}
