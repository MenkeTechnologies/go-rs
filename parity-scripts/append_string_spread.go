package main

import "fmt"

// `append(b, s...)` where `s` is a string is Go's one non-slice spread: it
// appends the string's bytes to a `[]byte`.

func main() {
	b := []byte("abc")
	b = append(b, "def"...)
	fmt.Println(string(b), len(b), b)

	var nilb []byte
	nilb = append(nilb, "hi"...)
	fmt.Println(string(nilb), len(nilb))

	empty := []byte("x")
	empty = append(empty, ""...)
	fmt.Println(string(empty), len(empty))

	utf := []byte{}
	utf = append(utf, "é中"...)
	fmt.Println(len(utf), utf, string(utf))

	// A variable, not a literal.
	suffix := "!!"
	b = append(b, suffix...)
	fmt.Println(string(b))

	// The ordinary slice spread still works.
	xs := []int{1, 2}
	ys := []int{3}
	fmt.Println(append(ys, xs...))
}
