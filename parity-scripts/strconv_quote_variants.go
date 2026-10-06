package main

// `strconv.Quote` and its ASCII / graphic variants, `CanBackquote`, `IsPrint`
// and `IsGraphic` — all decided by Go's own printable-rune tables, so an
// unassigned code point, a noncharacter and the soft hyphen are escaped — and
// the `%q` flags that reach them: `+` for ASCII-only, `#` for a raw string
// when `CanBackquote` allows one.

import (
	"fmt"
	"strconv"
)

func main() {
	strs := []string{"héllo ☺\n", "a`b", "tab\there", " nb ", "\x7f\x01", "quote\"'", "😀 𝄞", "͸­\U0010ffff﷐", "line\nbreak"}
	for _, s := range strs {
		fmt.Println(strconv.Quote(s), strconv.QuoteToASCII(s), strconv.QuoteToGraphic(s), strconv.CanBackquote(s))
		fmt.Printf("%q|%+q|%#q|%#+q|%10.3q|\n", s, s, s, s, s)
	}
	for _, r := range []rune{'a', '\'', '"', '☺', 0x2028, 0xa0, 0x1F600, -1, 0x110000, 0xD800, 0, 0x378, 0xE0100, 0x30000} {
		fmt.Println(strconv.QuoteRune(r), strconv.QuoteRuneToASCII(r), strconv.QuoteRuneToGraphic(r), strconv.IsPrint(r), strconv.IsGraphic(r))
		fmt.Printf("%q|%+q|%#q|%5q|%-5q|\n", r, r, r, r, r)
	}
	fmt.Printf("%q %+q\n", []string{"é", "x"}, []rune("é☺"))
	fmt.Printf("%q %+q\n", []byte("é"), "é")
}
