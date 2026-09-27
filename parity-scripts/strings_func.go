package main

import (
	"fmt"
	"strings"
	"unicode"
)

// `strings.Map` and the `*Func` family call the function they are handed.
func main() {
	drop := func(r rune) rune {
		switch {
		case r == 'x':
			return -1
		case r == 'a':
			return 'Å'
		}
		return unicode.ToUpper(r)
	}
	fmt.Println(strings.Map(drop, "banxana ünï"))
	fmt.Println(strings.Map(func(r rune) rune { return r + 1 }, "HAL"))
	fmt.Println(strings.Map(func(r rune) rune { return 0x110000 }, "ab"))
	isVowel := func(r rune) bool { return strings.ContainsRune("aeioué", r) }
	fmt.Println(strings.IndexFunc("rhythm éclat", isVowel), strings.LastIndexFunc("éclaté", isVowel))
	fmt.Println(strings.IndexFunc("xyz", isVowel), strings.LastIndexFunc("", isVowel))
	fmt.Println(strings.ContainsFunc("gym", isVowel), strings.ContainsFunc("gem", isVowel))
	fmt.Printf("%q\n", strings.TrimFunc("¡¡¡Hello, Gophers!!!", unicode.IsPunct))
	fmt.Printf("%q\n", strings.TrimLeftFunc("123abc456", unicode.IsDigit))
	fmt.Printf("%q\n", strings.TrimRightFunc("123abc456", unicode.IsDigit))
	fmt.Printf("%q %q\n", strings.TrimFunc("777", unicode.IsDigit), strings.TrimRightFunc("é9", unicode.IsDigit))
	f := strings.FieldsFunc("  foo1;bar2,baz3...", func(c rune) bool {
		return !unicode.IsLetter(c) && !unicode.IsNumber(c)
	})
	fmt.Printf("%q %d\n", f, len(f))
	fmt.Printf("%q %d\n", strings.FieldsFunc(";;", func(c rune) bool { return c == ';' }), 0)
}
