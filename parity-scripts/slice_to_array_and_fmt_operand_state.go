// `[N]T(s)` converts a slice to an array (a copy), panicking when the slice is
// shorter; and a `fmt` operand is printed with the state it has once *every*
// argument has been evaluated, so an argument that mutates it shows.
package main

import "fmt"

type pt struct{ X, Y int }

func main() {
	s := []int{7, 8, 9}
	a := [2]int([]int{7, 8, 9})
	s[0] = 100
	fmt.Println(a, s, [3]int(s) == [3]int{100, 8, 9})
	ps := []pt{{1, 2}, {3, 4}}
	pa := [2]pt(ps)
	ps[0].X = 50
	fmt.Println(pa, ps)
	fmt.Printf("%T %v\n", a, len([4]string([]string{"a", "b", "c", "d"})))
	func() {
		defer func() { fmt.Println(recover()) }()
		short := []int{1}
		fmt.Println([2]int(short))
	}()

	c := []int{1, 2, 3}
	fmt.Println(c, copy(c[1:], c))
	m := map[string]int{"a": 1}
	fmt.Println(m, func() int { m["b"] = 2; return len(m) }())
	bs := []byte("abc")
	fmt.Printf("%s %d\n", bs, copy(bs, "xyz"))
	fmt.Println(c, func() int { c[0] = -1; return 0 }())
}
