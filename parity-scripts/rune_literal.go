// A non-ASCII rune literal next to punctuation. The lexer's operator lookahead
// slices three bytes from the cursor, so `f('世')` puts a three-byte character
// two bytes past the `(` — slicing there panicked rather than lexing.
package main

import "fmt"

func main() {
	r := '世'
	fmt.Println(r, string(r))
	fmt.Println('é', 'a', '\n', '\'', '界')
	fmt.Printf("%c%c%c %q %U\n", '世', 'é', 'a', '界', '世')
	m := map[rune]string{'世': "world", 'é': "e"}
	fmt.Println(m['世'], m['é'], len(m))
	s := []rune{'世', '界'}
	fmt.Println(string(s), len(s), s[0] == '世')
	if '世' > 'a' {
		fmt.Println("gt", '世'-'a')
	}
	for _, c := range "aé世" {
		fmt.Printf("%d,", c)
	}
	fmt.Println()
}
