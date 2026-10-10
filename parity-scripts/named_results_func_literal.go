// A function literal's named results are zero-initialized locals: a bare
// `return` yields them, a deferred closure may rewrite them after the return
// value was set, and they are captured by reference like any local.
package main

import "fmt"

func main() {
	sum := func(xs ...int) (t int) {
		for _, x := range xs {
			t += x
		}
		return
	}
	fmt.Println(sum(1, 2, 3), sum(), sum([]int{4, 5}...))

	pair := func(a int) (r int, ok bool) {
		r = a
		ok = true
		return
	}
	fmt.Println(pair(4))

	bumped := func(a int) (r int) {
		defer func() { r++ }()
		return a
	}
	fmt.Println(bumped(1))

	ops := map[string]func(int) (out int, note string){
		"double": func(a int) (out int, note string) {
			out, note = a*2, "d"
			return
		},
		"neg": func(a int) (out int, note string) {
			defer func() { note += "!" }()
			return -a, "n"
		},
	}
	for _, k := range []string{"double", "neg"} {
		fmt.Println(ops[k](7))
	}

	recovered := func() (res string, err error) {
		defer func() {
			if r := recover(); r != nil {
				err = fmt.Errorf("recovered: %v", r)
			}
		}()
		var m map[string]int
		m["x"] = 1
		return "unreachable", nil
	}
	fmt.Println(recovered())

	zero := func() (a int, s string, f float64, b bool, sl []int, p *int) { return }
	fmt.Println(zero())
}
