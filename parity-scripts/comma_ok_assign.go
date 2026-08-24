// The three comma-ok forms — `m[k]`, `x.(T)`, `<-ch` — assigning to variables
// that already exist, into every kind of assignment target Go allows there.
package main

import "fmt"

type coBox struct {
	V  int
	OK bool
}

func main() {
	m := map[string]int{"a": 1, "b": 2}
	var v int
	var ok bool
	for _, k := range []string{"a", "z", "b"} {
		v, ok = m[k]
		fmt.Print(v, ok, " ")
	}
	fmt.Println()

	// targets that are not bare identifiers
	var b coBox
	b.V, b.OK = m["a"]
	fmt.Println(b)
	b.V, b.OK = m["nope"]
	fmt.Println(b)

	sl := make([]int, 2)
	flags := make([]bool, 2)
	sl[0], flags[0] = m["b"]
	sl[1], flags[1] = m["nope"]
	fmt.Println(sl, flags)

	out := map[string]int{}
	var got bool
	out["x"], got = m["a"]
	fmt.Println(out, got)

	// comma-ok type assertion with =
	var i any = "hello"
	var s string
	s, ok = i.(string)
	fmt.Printf("%q %v\n", s, ok)
	var num int
	num, ok = i.(int)
	fmt.Println(num, ok)

	// comma-ok receive with =
	ch := make(chan string, 2)
	ch <- "p"
	close(ch)
	var r string
	r, ok = <-ch
	fmt.Printf("%q %v\n", r, ok)
	r, ok = <-ch
	fmt.Printf("%q %v\n", r, ok)

	// blank targets
	_, ok = m["a"]
	fmt.Println(ok)
	v, _ = m["b"]
	fmt.Println(v)
}
