package main

// strings.ToUpper / ToLower / ToTitle map rune for rune with Go's simple
// case mappings (never lengthening a string: `ß` stays `ß`, `ᾳ` becomes `ᾼ`,
// no final-sigma rule), Title capitalizes after any separator, and EqualFold
// walks SimpleFold orbits (`ſ` folds to `S`, the Kelvin sign to `k`).

import (
	"fmt"
	"strings"
)

func main() {
	for _, s := range []string{"straße", "ǆemal", "İstanbul", "ΣΑΣ", "ﬁne", "Ǉ", "ŉ", "ΐ", "ᾳ", "Ꭰ", "aßc", "hello-world foo_bar 3rd", "élan vital", "x y z"} {
		fmt.Println(strings.ToUpper(s), strings.ToLower(s), strings.Title(s), strings.ToTitle(s), strings.EqualFold(s, strings.ToUpper(s)))
	}
	fmt.Println(strings.EqualFold("ſ", "S"), strings.EqualFold("K", "k"), strings.EqualFold("Straße", "STRASSE"), strings.EqualFold("Éclair", "éCLAIR"), strings.EqualFold("ab", "abc"))
}
