package main

import (
	"fmt"
	"strings"
)

func main() {
	for l := range strings.Lines("a\nbb\n\nc") {
		fmt.Printf("%q\n", l)
	}
	for p := range strings.SplitSeq("a,b,,c", ",") {
		fmt.Printf("%q ", p)
	}
	fmt.Println()
	for p := range strings.SplitAfterSeq("a,b,,c", ",") {
		fmt.Printf("%q ", p)
	}
	fmt.Println()
	for p := range strings.SplitSeq("héy", "") {
		fmt.Printf("%q ", p)
	}
	fmt.Println()
	for p := range strings.FieldsSeq("  a  b\tc\n ") {
		fmt.Printf("%q ", p)
	}
	fmt.Println()
	for p := range strings.FieldsFuncSeq("a1b22c", func(r rune) bool { return r >= '0' && r <= '9' }) {
		fmt.Printf("%q ", p)
		if p == "b" {
			break
		}
	}
	fmt.Println()
}
