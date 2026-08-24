// Edge cases for the same set: an empty subject, a multi-byte rune under the
// byte-indexed searches, every `ParseBool` spelling, and the `FormatFloat`
// verbs that switch notation.
package main

import (
	"fmt"
	"strconv"
	"strings"
)

func main() {
	// SplitN edge cases
	fmt.Printf("%q %q %q\n", strings.SplitN("", ",", 2), strings.SplitN("abc", "", 1), strings.SplitN("abc", "", 5))
	fmt.Printf("%q %q\n", strings.SplitN("a", "", 3), strings.SplitN("世界x", "", 2))
	fmt.Printf("%q %q\n", strings.SplitN(",a,", ",", 2), strings.SplitN("a", "b", 3))
	// Trim edge cases
	fmt.Printf("%q %q %q\n", strings.TrimLeft("", "x"), strings.TrimLeft("xxx", "x"), strings.TrimRight("abc", "cba"))
	// index families over multi-byte
	s := "aé世b"
	fmt.Println(strings.IndexByte(s, 'b'), strings.IndexRune(s, '世'), strings.IndexAny(s, "世"))
	fmt.Println(strings.LastIndexByte(s, 'a'), strings.ContainsRune(s, 'é'), strings.ContainsAny(s, ""))
	// FormatFloat corners
	fmt.Println(strconv.FormatFloat(0, 'f', -1, 64), strconv.FormatFloat(-0.5, 'f', 2, 64))
	fmt.Println(strconv.FormatFloat(1e-7, 'f', -1, 64), strconv.FormatFloat(1e-7, 'g', -1, 64))
	fmt.Println(strconv.FormatFloat(100000, 'g', -1, 64), strconv.FormatFloat(1e6, 'g', -1, 64))
	fmt.Println(strconv.FormatFloat(2.5, 'z', -1, 64))
	fmt.Println(strconv.FormatFloat(1.0/3.0, 'e', 10, 64), strconv.FormatFloat(255, 'e', 0, 64))
	fmt.Println(strconv.FormatFloat(float64(float32(1)/3), 'g', -1, 32))
	// ParseBool all ten spellings + the error path
	for _, in := range []string{"1", "t", "T", "TRUE", "true", "True", "0", "f", "F", "FALSE", "false", "False", "", "yes", "TrUe"} {
		v, err := strconv.ParseBool(in)
		fmt.Printf("%q=%v,%v ", in, v, err)
	}
	fmt.Println()
	fmt.Println(strconv.FormatBool(1 == 1), strconv.FormatBool(1 == 2))
	fmt.Println(strings.Compare("", ""), strings.Compare("ab", "abc"))
	fmt.Printf("%s %s %s\n", strconv.QuoteRune('a'), strconv.QuoteRune('\t'), strconv.QuoteRune('世'))
}
