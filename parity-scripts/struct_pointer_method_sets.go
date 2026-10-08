// A struct value and a pointer to it are distinct dynamic types: a type switch
// and an assertion tell `T` from `*T`, and the value's method set holds only
// its value-receiver methods — so `fmt` prints a value whose String/Error has
// a pointer receiver by its fields, and the value does not satisfy Stringer.
package main

import (
	"errors"
	"fmt"
)

type S struct{ A int }

func (s *S) String() string { return fmt.Sprint("S!", s.A) }

type V struct{ B int }

func (v V) String() string { return fmt.Sprint("V!", v.B) }

type E struct{ msg string }

func (e *E) Error() string { return e.msg }

type ValErr struct{ code int }

func (e ValErr) Error() string { return fmt.Sprint("code ", e.code) }

// Both: the value has String, the pointer additionally has Error, which fmt
// prefers.
type Both struct{}

func (Both) String() string { return "both-string" }
func (*Both) Error() string { return "both-error" }

// An embedded S promotes only its value methods into the value's set; an
// embedded *S promotes its pointer methods too.
type OuterVal struct {
	S
}

type OuterPtr struct {
	*S
}

func kind(v any) string {
	switch v.(type) {
	case *S:
		return "*S"
	case S:
		return "S"
	case fmt.Stringer:
		return "Stringer"
	}
	return "other"
}

func main() {
	s := S{1}
	fmt.Println(s, &s, V{2}, &V{3})
	fmt.Printf("%v %v %s %+v\n", s, &s, &s, s)
	fmt.Println(E{"e"}, &E{"p"}, ValErr{4}, &ValErr{5})
	fmt.Println(Both{}, &Both{})
	fmt.Println(kind(s), kind(&s), kind(V{1}), kind(&V{1}), kind(3))

	var x any = s
	_, isPtr := x.(*S)
	_, isVal := x.(S)
	_, isStr := x.(fmt.Stringer)
	var y any = &s
	_, yPtr := y.(*S)
	_, yVal := y.(S)
	_, yStr := y.(fmt.Stringer)
	fmt.Println(isPtr, isVal, isStr, yPtr, yVal, yStr)

	var o any = OuterVal{S{1}}
	_, oStr := o.(interface{ String() string })
	var op any = &OuterVal{}
	_, opStr := op.(fmt.Stringer)
	var ep any = OuterPtr{&S{9}}
	_, epStr := ep.(fmt.Stringer)
	fmt.Println(oStr, opStr, epStr)

	var err error = fmt.Errorf("wrap: %w", &E{"inner"})
	var target *E
	fmt.Println(errors.As(err, &target), target)
	var verr error = fmt.Errorf("wrap: %w", ValErr{7})
	var vt ValErr
	fmt.Println(errors.As(verr, &vt), vt.code)
}
