// Package unicode/utf8, vendored from Go's own utf8.go: decoding at both ends,
// encoding, counting, and validity over ASCII, multibyte and truncated input.
package main

import (
	"fmt"
	"unicode/utf8"
)

func main() {
	s := "a€日本"
	b := []byte(s)
	fmt.Println(len(s), utf8.RuneCountInString(s), utf8.RuneCount(b), utf8.ValidString(s), utf8.Valid(b))
	for i := 0; i < len(s); {
		r, size := utf8.DecodeRuneInString(s[i:])
		fmt.Printf("%d:%c/%d ", i, r, size)
		i += size
	}
	fmt.Println()
	r, size := utf8.DecodeLastRune(b)
	fmt.Println(string(r), size)
	r, size = utf8.DecodeLastRuneInString(s[:4])
	fmt.Println(string(r), size)
	// A truncated sequence decodes as RuneError, width 1.
	r, size = utf8.DecodeRune(b[1:3])
	fmt.Println(r == utf8.RuneError, size, utf8.FullRune(b[1:3]), utf8.FullRuneInString(s[1:]))
	fmt.Println(utf8.Valid(b[1:3]), utf8.RuneStart(b[1]), utf8.RuneStart(b[2]))
	buf := make([]byte, utf8.UTFMax)
	for _, r := range []rune{'A', 'é', '€', '😀', utf8.MaxRune + 1, 0xD800} {
		n := utf8.EncodeRune(buf, r)
		fmt.Println(utf8.RuneLen(r), n, buf[:n], utf8.ValidRune(r), string(utf8.AppendRune(nil, r)))
	}
	r, size = utf8.DecodeRune(nil)
	fmt.Println(r, size, utf8.RuneSelf, utf8.RuneError, utf8.MaxRune)
}
