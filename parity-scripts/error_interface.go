// `error` is `interface{ Error() string }`, not the empty interface: a type
// switch or assertion against it tests the method set, and the conversion panic
// it raises names the interface and the missing method the way Go's does.
package main

import (
	"errors"
	"fmt"
)

type eiErr struct{ m string }

func (e eiErr) Error() string { return "my:" + e.m }

type eiStringer interface{ String() string }

type eiNotErr struct{}

func (eiNotErr) String() string { return "ns" }

func eiKind(i any) string {
	switch v := i.(type) {
	case int:
		return fmt.Sprintf("int:%d", v+1)
	case error:
		return "err:" + v.Error()
	case eiStringer:
		return "str:" + v.String()
	case string, bool:
		return fmt.Sprintf("sb:%v", v)
	default:
		return fmt.Sprintf("other:%T", v)
	}
}

func eiTry(f func()) {
	defer func() { fmt.Println("rec:", recover()) }()
	f()
}

func main() {
	fmt.Println(eiKind(1))
	fmt.Println(eiKind(2.5))
	fmt.Println(eiKind("s"))
	fmt.Println(eiKind(true))
	fmt.Println(eiKind(eiErr{"x"}))
	fmt.Println(eiKind(errors.New("e")))
	fmt.Println(eiKind(eiNotErr{}))
	fmt.Println(eiKind([]int{1}))
	fmt.Println(eiKind(nil))

	var a any = eiErr{"y"}
	e, ok := a.(error)
	fmt.Println(e, ok)
	var b any = 7
	_, ok2 := b.(error)
	fmt.Println(ok2)
	fmt.Println(errors.Is(fmt.Errorf("w: %w", eiErr{"z"}), eiErr{"z"}))

	eiTry(func() { _ = b.(error) })
	eiTry(func() { _ = b.(string) })
	eiTry(func() { _ = b.(eiStringer) })
	var n any
	eiTry(func() { _ = n.(error) })
	eiTry(func() { _ = n.(int) })
}
