package main

// Slice expressions are bounds-checked the way Go's compiler emits the checks:
// high bound against capacity (an array or string says "length"), then low
// against high, three-index forms from `max` down, a negative bound reported
// without the limit. Each panic is a recoverable `runtime error`, and an index
// below zero drops the "with length" half too.

import "fmt"

func try(name string, f func()) {
	defer func() {
		if r := recover(); r != nil {
			fmt.Println(name, "->", r)
		}
	}()
	f()
	fmt.Println(name, "ok")
}

func main() {
	s := make([]int, 2, 3)
	arr := [4]int{1, 2, 3, 4}
	str := "hello"
	n, m, neg := 5, 1, -1
	try("s[1:5]", func() { _ = s[1:n] })
	try("s[:3]", func() { fmt.Println(len(s[:3]), cap(s[:3])) })
	try("s[2:1]", func() { _ = s[2:m] })
	try("s[-1:]", func() { _ = s[neg:] })
	try("s[:-1]", func() { _ = s[:neg] })
	try("s[3:]", func() { _ = s[3:] })
	try("s[4:]", func() { _ = s[n-1:] })
	try("s[0:1:5]", func() { _ = s[0:1:n] })
	try("s[0:3:2]", func() { _ = s[0:3:n-3] })
	try("s[2:1:3]", func() { _ = s[2:m:3] })
	try("s[::-1]", func() { _ = s[0:0:neg] })
	try("arr[1:5]", func() { _ = arr[1:n] })
	try("arr[1:3:5]", func() { _ = arr[1:3:n] })
	try("str[2:9]", func() { _ = str[2 : n+4] })
	try("str[4:2]", func() { _ = str[4 : n-3] })
	try("str[6:]", func() { _ = str[n+1:] })
	try("str[1:3]", func() { fmt.Println(str[1:3]) })
	try("s[-1]", func() { _ = s[neg] })
	try("s[-1]=", func() { s[neg] = 1 })
	try("str[-1]", func() { _ = str[neg] })
	t := s[1:2]
	try("t[:2]", func() { fmt.Println(t[:2], cap(t)) })
	try("t[:3]", func() { _ = t[:n-2] })
}
