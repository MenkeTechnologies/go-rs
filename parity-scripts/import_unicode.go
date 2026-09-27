package main

import (
	"fmt"
	"unicode"
)

// `unicode` runs from its real source. It declares defined non-struct types
// (`SpecialCase`, `d`) that a package-level var names before their
// declaration, which the linker has to qualify and merge like struct types.
func main() {
	for _, r := range []rune{'A', 'z', 'Ⓐ', 'ß', 'é', 'Σ', 'ς', '5', '٣', ' ', '\t', '!', '中', 'ǅ', 0x1F600, 'İ'} {
		fmt.Println(string(r), unicode.IsUpper(r), unicode.IsLower(r), unicode.IsLetter(r), unicode.IsDigit(r),
			unicode.IsNumber(r), unicode.IsSpace(r), unicode.IsPunct(r), unicode.IsPrint(r), unicode.IsTitle(r),
			string(unicode.ToUpper(r)), string(unicode.ToLower(r)), string(unicode.ToTitle(r)), unicode.SimpleFold(r),
			unicode.Is(unicode.Han, r), unicode.In(r, unicode.Greek, unicode.Latin), unicode.IsSymbol(r))
	}
	fmt.Println(unicode.MaxRune, unicode.ReplacementChar, unicode.Version)
	fmt.Println(unicode.TurkishCase.ToUpper('i'), unicode.TurkishCase.ToLower('I'))
}
