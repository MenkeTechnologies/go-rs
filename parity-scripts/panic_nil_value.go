// `panic(nil)` panics with a `*runtime.PanicNilError` (Go 1.21): `recover`
// returns an `error` whose text carries the `runtime error: ` prefix.
package main

import "fmt"

func try() (err error) {
	defer func() {
		r := recover()
		fmt.Println(r)
		fmt.Printf("%T\n", r)
		err = r.(error)
	}()
	panic(nil)
}

func main() {
	err := try()
	fmt.Println(err.Error())
	var e error
	defer func() {
		r := recover()
		_, isErr := r.(error)
		fmt.Println(r, isErr)
	}()
	panic(e)
}
