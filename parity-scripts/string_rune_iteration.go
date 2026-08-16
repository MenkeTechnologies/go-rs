package main

// Ranging a string yields byte offsets and decoded runes, so a multi-byte code
// point advances the index by its encoded width and skips the offsets in
// between. Indexing the same string yields bytes instead, and the two
// conversions produce slices whose element types `%T` must keep apart.

import (
	"fmt"
	"strings"
)

func main() {
	s := "aé中z"

	for i, r := range s {
		fmt.Printf("%d:%d:%q ", i, r, r)
	}
	fmt.Println()

	fmt.Println(len(s), len([]rune(s)), len([]byte(s)))

	// Indexing is bytewise; ranging is runewise.
	for i := 0; i < len(s); i++ {
		fmt.Printf("%d ", s[i])
	}
	fmt.Println()
	for _, r := range []rune(s) {
		fmt.Printf("%d ", r)
	}
	fmt.Println()

	// A slice expression cuts bytes, but on a boundary here: a cut that splits
	// a code point yields invalid UTF-8, which go-rs cannot represent (BUGS.md).
	fmt.Printf("%q %q %q\n", s[0:1], s[1:3], s[3:6])

	b := []byte(s)
	fmt.Println(string(b[0:3]), len(b))

	fmt.Printf("%T %T %T %T\n", []rune(s), []byte(s), []rune(s)[0], []byte(s)[0])
	fmt.Printf("%c %c %U %U\n", s[0], 0x4e2d, 'a', '中')

	fmt.Println(string(rune(65)), string(rune(0x4e2d)))
	fmt.Println(strings.ToUpper("héllo"), strings.Count(s, "é"), strings.Index(s, "中"))

	// An empty string ranges zero times.
	n := 0
	for range "" {
		n++
	}
	fmt.Println("empty", n)

	// A rune appended as a string keeps its encoded width.
	acc := ""
	for _, r := range "xé中" {
		acc += string(r)
	}
	fmt.Println(acc, len(acc), len([]rune(acc)))
}
