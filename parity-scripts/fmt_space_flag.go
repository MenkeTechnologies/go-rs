package main

// The space flag leaves room for the sign a non-negative number elides, and `+`
// outranks it. On a string or byte slice printed in hex it means something else
// entirely: it separates the bytes.

import "fmt"

func main() {
	fmt.Printf("[% d][% d][%+ d][% 5d][%- 5d|]\n", 42, -42, 42, 42, 42)
	fmt.Printf("[% f][% f][% .2f][% e]\n", 1.5, -1.5, 1.5, 1.5)
	fmt.Printf("[% x][% X][% x]\n", 255, 255, -255)
	fmt.Printf("[% o][% b]\n", 8, 5)
	fmt.Printf("[% v][% v]\n", 42, -42)
	fmt.Printf("[% d]\n", []int{1, -2})
	fmt.Printf("[% x][% x]\n", "abc", []byte("abc"))
	fmt.Printf("[% q][% s][% t]\n", "a", "a", true)
	fmt.Printf("[% 05d][% 05d]\n", 42, -42)
	fmt.Printf("[% U][% c]\n", 65, 65)
	fmt.Printf("[% g]\n", 1.5)
}
