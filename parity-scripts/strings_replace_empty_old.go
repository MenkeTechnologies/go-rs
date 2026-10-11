package main

import (
	"bytes"
	"fmt"
	"strings"
)

// strings.Replace with an empty old string matches at the start and after
// every rune, inserting new between the runes up to n times, and n caps the
// replacements at every count (strings.go's Replace).
func main() {
	for _, n := range []int{-1, 0, 1, 2, 3, 4, 5, 9} {
		fmt.Printf("%d %q %q %q\n", n, strings.Replace("héy", "", "-", n), strings.Replace("", "", "+", n), strings.Replace("aaa", "a", "bb", n))
	}
	fmt.Println(strings.ReplaceAll("日本", "", "|"), strings.ReplaceAll("x", "x", "x"), strings.Replace("abab", "ab", "", -1))
	fmt.Printf("%q %q\n", bytes.Replace([]byte("ab"), nil, []byte("."), -1), bytes.ReplaceAll([]byte("ab"), []byte(""), []byte("_")))
}
