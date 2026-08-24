// The `strings` and `strconv` functions added alongside the originals, and the
// corners each gets wrong when it is written from the name rather than the
// implementation: SplitN's last part holds the remainder whole, an empty
// separator explodes into runes, and `FormatFloat` at `bitSize` 32 answers at
// float32's shorter round-trip.
package main

import (
	"fmt"
	"strconv"
	"strings"
)

func main() {
	s := "Hello, 世界! Hello"
	fmt.Println(strings.ContainsRune(s, '世'), strings.ContainsRune(s, 'z'))
	fmt.Println(strings.ContainsAny(s, "xyz!"), strings.ContainsAny(s, "xyz"))
	fmt.Println(strings.IndexByte(s, 'e'), strings.IndexByte(s, 'z'))
	fmt.Println(strings.IndexRune(s, '世'), strings.IndexRune(s, 'z'))
	fmt.Println(strings.IndexAny(s, "界o"), strings.IndexAny(s, "xyz"))
	fmt.Println(strings.LastIndexByte(s, 'l'), strings.LastIndexByte(s, 'z'))
	fmt.Println(strings.SplitN("a,b,c,d", ",", 2), strings.SplitN("a,b,c", ",", -1))
	fmt.Println(strings.SplitN("a,b", ",", 0) == nil, len(strings.SplitN("a,b", ",", 0)))
	fmt.Println(strings.SplitN("abc", "", 2), strings.SplitN("a,b,c", ",", 10))
	fmt.Printf("%q %q\n", strings.TrimLeft("xxhixx", "x"), strings.TrimRight("xxhixx", "x"))
	fmt.Println(strings.Compare("a", "b"), strings.Compare("b", "a"), strings.Compare("a", "a"))

	b, err := strconv.ParseBool("true")
	fmt.Println(b, err)
	b, err = strconv.ParseBool("0")
	fmt.Println(b, err)
	b, err = strconv.ParseBool("yes")
	fmt.Println(b, err)
	fmt.Println(strconv.FormatBool(true), strconv.FormatBool(false))

	fmt.Println(strconv.FormatFloat(3.1415926535, 'f', 4, 64))
	fmt.Println(strconv.FormatFloat(3.1415926535, 'f', -1, 64))
	fmt.Println(strconv.FormatFloat(1e21, 'f', -1, 64))
	fmt.Println(strconv.FormatFloat(0.1, 'f', -1, 32))
	fmt.Println(strconv.FormatFloat(1234.5678, 'e', 3, 64))
	fmt.Println(strconv.FormatFloat(1234.5678, 'E', -1, 64))
	fmt.Println(strconv.FormatFloat(1234.5678, 'g', -1, 64))
	fmt.Println(strconv.FormatFloat(1234.5678, 'G', 4, 64))
	fmt.Println(strconv.FormatFloat(0.1, 'g', -1, 32))
	fmt.Println(strconv.FormatFloat(1.0, 'f', -1, 64))
	fmt.Printf("%q\n", strconv.QuoteRune('世'))
	fmt.Printf("%q\n", strconv.QuoteRune('\n'))
}
