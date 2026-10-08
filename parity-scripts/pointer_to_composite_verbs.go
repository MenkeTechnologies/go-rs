// A pointer to a slice, array or map under a non-`%v` verb: `fmt` writes the
// `&` and renders the pointee with the verb, element by element, and a `*[]int`
// keeps its element type (so `%x` lists numbers, not bytes).
package main

import "fmt"

type Ints []int

func main() {
	s := []int{1, 2}
	p := &s
	fmt.Printf("%v %d %x %+v\n", p, p, p, p)
	fmt.Printf("%x\n", &[]int{3, 4})
	fmt.Printf("%d %v %o\n", &[2]int{3, 4}, &[2]int{3, 4}, &[2]int{8, 9})
	fmt.Printf("%d %x\n", &map[string]int{"a": 10}, &map[int]int{255: 16})
	fmt.Printf("%d %+d\n", &struct{ A int }{1}, &struct{ B int }{2})
	b := []byte("ab")
	fmt.Printf("%s %q %x\n", &b, &[]string{"c"}, &b)
	fmt.Printf("%d %5d|\n", &Ints{7, 8}, &[]int{1})
}
