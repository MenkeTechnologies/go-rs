package main

import (
	"errors"
	"fmt"
	"strconv"
)

func main() {
	for _, s := range []string{`"a\tb"`, `'x'`, "`raw\\n`", `"é\U0001F600\x41\101"`, `"unterminated`, `'ab'`, `''`, `"bad\q"`, `"é"`, `'é'`, `"a"b`, `x`, `"\'"`, `'\"'`, "`a\rb`", `"\ud800"`, `"\777"`} {
		u, err := strconv.Unquote(s)
		fmt.Printf("%-24q -> %q %v %v\n", s, u, err, errors.Is(err, strconv.ErrSyntax))
	}
	p, err := strconv.QuotedPrefix(`"hello" world`)
	fmt.Println(p, err)
	p, err = strconv.QuotedPrefix(`nope`)
	fmt.Printf("%q %v\n", p, err)
	v, mb, tail, err := strconv.UnquoteChar(`☺ rest`, '"')
	fmt.Println(v, mb, tail, err)
	v, mb, tail, err = strconv.UnquoteChar(`é!`, 0)
	fmt.Println(v, mb, tail, err)
	b := []byte("v=")
	b = strconv.AppendInt(b, -42, 10)
	b = append(b, ' ')
	b = strconv.AppendUint(b, 255, 16)
	b = append(b, ' ')
	b = strconv.AppendBool(b, true)
	b = append(b, ' ')
	b = strconv.AppendFloat(b, 3.25, 'g', -1, 64)
	b = append(b, ' ')
	b = strconv.AppendQuote(b, "q\"")
	b = strconv.AppendQuoteRune(b, '☺')
	b = strconv.AppendQuoteToASCII(b, "☺")
	b = strconv.AppendQuoteRuneToASCII(b, '☺')
	b = strconv.AppendQuoteToGraphic(b, " ")
	b = strconv.AppendQuoteRuneToGraphic(b, '\t')
	fmt.Println(string(b))
	f := strconv.Unquote
	fmt.Println(f(`"via value"`))
}
