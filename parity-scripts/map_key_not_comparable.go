package main

// A slice is not a comparable type, so it cannot be a map key, and `go` refuses
// to build the program rather than diagnosing it at run time. go-rs rejects it
// the same way — the corpus file that pins the *exit status* against the
// reference, since a rejected program prints nothing for stdout to compare.
// The other not-comparable shapes (a map, a func, a struct or defined type
// built out of one) are covered in tests/eval.rs, which can carry more than one
// program.

import "fmt"

func main() {
	m := make(map[[]int]int)
	fmt.Println(len(m))
}
