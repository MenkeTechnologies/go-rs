package main

// Indexing a string yields a `byte` and ranging over one yields a `rune`, so
// arithmetic on either wraps at that width — `s[0] - 200` is a `uint8`, and a
// range value pushed past `MaxInt32` comes back negative — and `%T` names the
// width rather than `int`.

import "fmt"

func main() {
	s := "abc"
	b := s[0]
	b -= 200
	fmt.Println(b)
	fmt.Println(s[0]-'b', s[1]+200, "xyz"[2]+10)
	for i, c := range s {
		c += 0x7fffffff
		fmt.Println(i, c)
	}
	for _, c := range "é世" {
		fmt.Printf("%T %v %c\n", c, c, c)
	}
	fmt.Printf("%T %T\n", s[0], "lit"[1])
	d := s[2] - s[0]
	fmt.Printf("%T %v\n", d, d)
	var word string = "zz"
	sum := word[0] + word[1]
	fmt.Println(sum)
}
