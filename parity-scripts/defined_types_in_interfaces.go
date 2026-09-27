package main

// A defined non-struct type stored in an interface keeps its dynamic type:
// methods dispatch, type switches and assertions match it, `==` and map keys
// compare type and value, and `fmt` prints it through String() / Error().

import (
	"errors"
	"fmt"
)

type Celsius float64
type Code int
type ErrCode string

func (e ErrCode) Error() string { return "code:" + string(e) }

func (c Celsius) String() string { return fmt.Sprintf("%.1fC", float64(c)) }

func check(n int) error {
	if n < 0 {
		return ErrCode("neg")
	}
	return nil
}

func describe(v any) string {
	switch x := v.(type) {
	case Celsius:
		return "celsius " + fmt.Sprint(float64(x)+1)
	case Code:
		return fmt.Sprintf("code %d", int(x)*2)
	case int:
		return "int"
	case fmt.Stringer:
		return "stringer"
	}
	return "other"
}

func main() {
	var a any = Code(3)
	fmt.Printf("%T %v %d\n", a, a, a)
	fmt.Println(a == Code(3), a == 3, a != Code(4))
	m := map[any]string{Code(1): "code", 1: "int"}
	fmt.Println(len(m), m[Code(1)], m[1])
	fmt.Println(describe(Celsius(20)), describe(Code(5)), describe(7), describe("s"))
	err := check(-1)
	fmt.Println(err, err != nil)
	var ec ErrCode
	fmt.Println(errors.As(err, &ec), ec)
	fmt.Println(check(1) == nil)
	c, ok := a.(Code)
	fmt.Println(c+1, ok)
	_, ok2 := a.(int)
	fmt.Println(ok2)
	ch := make(chan any, 2)
	ch <- Celsius(1.5)
	v := <-ch
	fmt.Println(v)
	xs := []any{Code(1), Celsius(2), "s"}
	fmt.Println(xs)
	fmt.Printf("%v %+v\n", xs, xs)
	temps := []Celsius{1, 2.5}
	fmt.Println(temps, len(temps))
	fmt.Printf("%d %s %v\n", Code(9), Celsius(3), []Code{1, 2})
	var s fmt.Stringer = Celsius(4)
	fmt.Println(s, s.String())
	type T struct{ V any }
	t := T{Code(8)}
	fmt.Printf("%v %T\n", t.V, t.V)
	fmt.Printf("%#v\n", Code(2))
}
