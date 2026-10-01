// A func held in a map or slice that is itself a struct field is indexed
// and then called, not mistaken for a method or a generic instantiation.
package main

import (
	"fmt"
	"strings"
)

type reg struct {
	handlers map[string]func(string) string
	steps    []func(int) int
}

func main() {
	r := reg{
		handlers: map[string]func(string) string{
			"up":   strings.ToUpper,
			"bang": func(s string) string { return s + "!" },
		},
		steps: []func(int) int{
			func(x int) int { return x + 1 },
			func(x int) int { return x * 10 },
		},
	}
	fmt.Println(r.handlers["up"]("hey"), r.handlers["bang"]("hey"))
	v := 4
	for i := range r.steps {
		v = r.steps[i](v)
	}
	fmt.Println(v, r.steps[1](r.steps[0](1)))
	p := &r
	fmt.Println(p.handlers["bang"]("ptr"))
}
