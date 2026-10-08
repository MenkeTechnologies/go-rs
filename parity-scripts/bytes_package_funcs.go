// Package-level `bytes` functions: comparison, search, split/fields/join,
// case mapping, trimming (including the nil results Go returns when nothing
// is left), replacement and the Cut family — and that the subslices they
// return alias the argument.
package main

import (
	"bytes"
	"fmt"
)

func main() {
	a, b := []byte("abc"), []byte("abd")
	fmt.Println(bytes.Equal(a, []byte("abc")), bytes.Equal(nil, []byte{}), bytes.Compare(a, b), bytes.Compare(b, a), bytes.Compare(nil, nil))

	s := []byte("chicken kitchen")
	fmt.Println(bytes.Index(s, []byte("ken")), bytes.LastIndex(s, []byte("chen")), bytes.IndexByte(s, 'k'), bytes.LastIndexByte(s, 'k'))
	fmt.Println(bytes.Contains(s, []byte("kit")), bytes.ContainsAny(s, "xyz"), bytes.ContainsRune([]byte("héllo"), 'é'), bytes.IndexRune([]byte("héllo"), 'l'))
	fmt.Println(bytes.IndexAny(s, "nk"), bytes.Count(s, []byte("en")), bytes.Count([]byte("héllo"), nil))
	fmt.Println(bytes.IndexFunc([]byte("ab1"), func(r rune) bool { return r >= '0' && r <= '9' }), bytes.ContainsFunc(s, func(r rune) bool { return r == 'z' }))
	fmt.Println(bytes.HasPrefix(s, []byte("chi")), bytes.HasSuffix(s, []byte("hen")), bytes.HasPrefix(s, nil))

	fmt.Printf("%q\n", bytes.Split([]byte("a,b,,c"), []byte(",")))
	fmt.Printf("%q\n", bytes.SplitN([]byte("a,b,c,d"), []byte(","), 2))
	fmt.Printf("%q\n", bytes.SplitAfter([]byte("a,b,c"), []byte(",")))
	fmt.Printf("%q\n", bytes.Split([]byte("héy"), nil))
	fmt.Println(bytes.SplitN([]byte("x"), []byte(","), 0) == nil)
	fs := bytes.Fields([]byte("  foo bar\t baz\n "))
	fmt.Printf("%q %d\n", fs, len(bytes.Fields([]byte("   "))))
	fmt.Printf("%q\n", bytes.Fields([]byte("a b c d")))
	fmt.Printf("%s\n", bytes.Join([][]byte{[]byte("x"), []byte("y"), []byte("z")}, []byte(", ")))
	fmt.Printf("%q %q\n", bytes.Join(nil, []byte("-")), bytes.Repeat([]byte("ab"), 3))

	fmt.Printf("%s %s %s %s\n", bytes.ToUpper([]byte("Hello, wörld")), bytes.ToLower([]byte("HeLLo")), bytes.ToUpper([]byte("ABC")), bytes.ToTitle([]byte("go")))
	fmt.Printf("%q %q %q\n", bytes.TrimSpace([]byte("  \t hi there \n")), bytes.Trim([]byte("xxhixx"), "x"), bytes.TrimLeft([]byte("aabca"), "ab"))
	fmt.Printf("%q %q %q\n", bytes.TrimRight([]byte("aabca"), "ac"), bytes.TrimPrefix([]byte("prefix-x"), []byte("prefix-")), bytes.TrimSuffix([]byte("a.go"), []byte(".go")))
	fmt.Println(bytes.TrimSpace([]byte("   ")) == nil, bytes.Trim([]byte("xx"), "x") == nil, bytes.TrimLeft(nil, "x") == nil, bytes.TrimRight([]byte("xx"), "x") == nil)
	fmt.Printf("%q\n", bytes.TrimSpace([]byte("  wide  ")))

	fmt.Printf("%s|%s|%s\n", bytes.Replace([]byte("oink oink oink"), []byte("k"), []byte("ky"), 2), bytes.ReplaceAll([]byte("oink oink"), []byte("oink"), []byte("moo")), bytes.Replace([]byte("hé"), nil, []byte("-"), -1))
	fmt.Println(bytes.EqualFold([]byte("Go"), []byte("GO")), bytes.Runes([]byte("hé")), string(bytes.Map(func(r rune) rune { return r + 1 }, []byte("abc"))))

	before, after, found := bytes.Cut([]byte("key=value"), []byte("="))
	fmt.Printf("%s %s %v\n", before, after, found)
	_, after, found = bytes.Cut([]byte("novalue"), []byte("="))
	fmt.Println(after == nil, found)
	rest, ok := bytes.CutPrefix([]byte("v1.2"), []byte("v"))
	head, ok2 := bytes.CutSuffix([]byte("v1.2"), []byte(".3"))
	fmt.Printf("%s %v %s %v\n", rest, ok, head, ok2)
	fmt.Println(bytes.Clone(nil) == nil, bytes.Clone([]byte("q")))

	// Subslices alias the argument; the copying functions do not.
	buf := []byte("one two")
	parts := bytes.Fields(buf)
	parts[1][0] = 'T'
	trimmed := bytes.TrimSpace(buf)
	trimmed[0] = 'O'
	up := bytes.ToUpper(buf)
	up[0] = '!'
	fmt.Printf("%s %s %d\n", buf, up, cap(parts[0]))
}
