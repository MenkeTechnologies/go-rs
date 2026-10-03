package main

import (
	"fmt"
	"strings"
)

func main() {
	// Cut / CutPrefix / CutSuffix return several results, destructured.
	for _, kv := range []string{"k=v", "kv", "=v", "a=b=c"} {
		k, v, ok := strings.Cut(kv, "=")
		fmt.Printf("%q %q %v\n", k, v, ok)
	}
	rest, ok := strings.CutPrefix("foobar", "foo")
	fmt.Println(rest, ok)
	rest, ok = strings.CutPrefix("foobar", "bar")
	fmt.Println(rest, ok)
	rest, ok = strings.CutSuffix("foobar", "bar")
	fmt.Println(rest, ok)
	rest, ok = strings.CutSuffix("foobar", "")
	fmt.Println(rest, ok)
	if before, _, found := strings.Cut("user@example.com", "@"); found {
		fmt.Println(before)
	}

	// SplitAfter keeps the separator; n bounds the pieces.
	fmt.Printf("%q\n", strings.SplitAfter("a,b,c", ","))
	fmt.Printf("%q\n", strings.SplitAfter("a,b,", ","))
	fmt.Printf("%q\n", strings.SplitAfter("héllo", ""))
	fmt.Printf("%q\n", strings.SplitAfterN("a,b,c,d", ",", 2))
	fmt.Printf("%q\n", strings.SplitAfterN("abc", "", 2))
	fmt.Println(strings.SplitAfterN("a,b", ",", 0) == nil)

	fmt.Println(strings.LastIndexAny("golang", "gl"), strings.LastIndexAny("héllo", "é"), strings.LastIndexAny("go", ""), strings.LastIndexAny("go", "xyz"))

	// Replacer: matches in the order they appear, earliest argument first
	// at a position, no overlaps.
	r := strings.NewReplacer("<", "&lt;", ">", "&gt;", "&", "&amp;")
	fmt.Println(r.Replace("<a & b>"), r.Replace("plain"))
	fmt.Println(strings.NewReplacer("a", "1", "b", "2").Replace("abcab"))
	fmt.Println(strings.NewReplacer("aaa", "3", "aa", "2", "a", "1").Replace("aaaa"))
	fmt.Println(strings.NewReplacer("a", "1", "aaa", "3").Replace("aaaa"))
	fmt.Println(strings.NewReplacer("", "-").Replace("abc"))
	fmt.Println(strings.NewReplacer("é", "e", "ü", "u").Replace("café über"))
	fmt.Println(strings.NewReplacer("hello", "bye").Replace("hello, hello world"))
	fmt.Println(strings.NewReplacer().Replace("unchanged"))
	var rp *strings.Replacer = strings.NewReplacer("x", "y")
	fmt.Println(rp.Replace("xox"))
	defer func() { fmt.Println("recovered:", recover()) }()
	strings.NewReplacer("odd")
}
