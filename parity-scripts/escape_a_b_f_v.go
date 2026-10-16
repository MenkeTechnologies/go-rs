package main

import (
	"fmt"
	"strconv"
)

func main() {
	s := "\a\b\f\v"
	fmt.Println(len(s), []byte(s))
	r := []rune{'\a', '\b', '\f', '\v', '\r', '\t', '\n', '\\', '\'', '\x41', '\101', 'é', '\U0001F600'}
	fmt.Println(r)
	fmt.Println(strconv.Quote("\a\b\f\v\r\t\n\\\"\x00\x1f\x7f"))
	fmt.Printf("%q %q\n", "\a\b\f\v", '\a')
	fmt.Println(strconv.QuoteRune('\a'), strconv.QuoteToASCII("\aé"))
	fmt.Println([]byte("\a\b\f\v\\\""))
}
