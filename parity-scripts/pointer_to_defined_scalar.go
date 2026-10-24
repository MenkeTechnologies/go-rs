package main

import (
	"errors"
	"fmt"
)

// A pointer to a defined scalar type is its own dynamic type in an interface:
// `*Celsius`, with Go's method set for `*T` (value and pointer methods), apart
// from `Celsius`'s (value methods only).

type Celsius float64

func (c Celsius) String() string { return fmt.Sprintf("%.1fC", float64(c)) }
func (c *Celsius) Set(v float64) { *c = Celsius(v) }

type Count int

func (n *Count) Inc()           { *n++ }
func (n *Count) String() string { return fmt.Sprintf("count=%d", int(*n)) }

type Code int

func (c *Code) Error() string { return fmt.Sprintf("code %d", int(*c)) }

type Name string
type Flag bool
type Small int8

type Incer interface{ Inc() }

func newCount() *Count { c := Count(10); return &c }

func asIncer(p *Count) Incer { return p }

func fail(n int) error {
	c := Code(n)
	return &c
}

func describe(x any) string {
	switch v := x.(type) {
	case *Count:
		v.Inc()
		return fmt.Sprintf("*Count %d", int(*v))
	case Count:
		return fmt.Sprintf("Count %d", int(v))
	case *Celsius:
		return "ptr " + v.String()
	case Celsius:
		return "val " + v.String()
	}
	return "other"
}

func main() {
	// %T, type switches and assertions see the `*`.
	var c Celsius = 3
	var a any = &c
	fmt.Printf("%T %T %T\n", &c, a, c)
	fmt.Println(describe(&c), describe(c))
	_, ok := a.(Celsius)
	p, ok2 := a.(*Celsius)
	fmt.Println(ok, ok2, *p)

	// Dispatch through the pointer: a pointer method writes the variable, a
	// value method reads what it holds.
	var s fmt.Stringer = &c
	var st interface{ Set(float64) } = &c
	st.Set(21.5)
	fmt.Println(c, s.String(), s)
	fmt.Printf("%T\n", st)

	// Method sets: only the pointer has the pointer-receiver methods.
	var k Count = 5
	fmt.Println(k, &k)
	fmt.Printf("%v %s %d\n", &k, &k, k)
	var i Incer = &k
	i.Inc()
	fmt.Println(k)
	fmt.Println(describe(&k))
	fmt.Println(describe(k))
	_, okv := any(k).(Incer)
	_, okp := any(&k).(Incer)
	_, oks := any(k).(fmt.Stringer)
	fmt.Println(okv, okp, oks)

	// Pointers from a call, a parameter, new and a slice element.
	q := newCount()
	asIncer(q).Inc()
	fmt.Println(*q)
	n := new(Count)
	fmt.Printf("%T %T\n", q, n)
	var in Incer = n
	in.Inc()
	fmt.Println(*n)
	xs := []*Count{&k, n}
	fmt.Println(xs[0].String(), xs[1].String(), xs)

	// Interface equality compares the pointers.
	d := c
	var a2 any = &c
	fmt.Println(a == a2, a == any(&d))

	// An error whose Error method has a pointer receiver.
	err := fail(7)
	fmt.Printf("%T %v\n", err, err)
	var target *Code
	fmt.Println(errors.As(err, &target), int(*target))

	// fmt's scanner stores through the pointer by the underlying kind.
	var nm Name
	var fl Flag
	var sm Small
	cnt, serr := fmt.Sscan("hello true -7 4.25", &nm, &fl, &sm, &c)
	fmt.Println(cnt, serr, nm, fl, sm, c)
	cnt, serr = fmt.Sscan("300", &sm)
	fmt.Println(cnt, serr, sm)
	cnt, serr = fmt.Sscan("abc", &k)
	fmt.Println(cnt, serr, int(k))
	cnt, serr = fmt.Sscanf("12 2.5", "%d %g", &k, &c)
	fmt.Println(cnt, serr, int(k), c)
	cnt, serr = fmt.Sscan("x", nm)
	fmt.Println(cnt, serr)
}
