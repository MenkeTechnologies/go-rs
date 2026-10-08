// `string(b)` of a nil `[]byte` / `[]rune` that arrived as an untyped `nil`
// argument is the empty string, as it is for a declared nil slice.
package main

import "fmt"

func conv(b []byte) string { return string(b) }

func runes(r []rune) string { return string(r) }

func main() {
	var x []byte
	fmt.Println(string(x) == "", len(string(x)), conv(nil) == "", len(conv(nil)), conv([]byte{}) == "")
	fmt.Printf("%q %q %q\n", conv(nil), runes(nil), string(x))
	fmt.Println(conv(nil)+"|"+runes(nil)+"|", conv(nil) == runes(nil))
}
