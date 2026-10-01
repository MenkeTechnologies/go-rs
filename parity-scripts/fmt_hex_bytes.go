// %x / %X of a byte slice encode the bytes themselves, including ones that
// are not UTF-8 on their own; a precision limits the bytes encoded, the space
// flag separates them, and # prefixes 0x once — or before every byte with
// the space flag.
package main

import "fmt"

type digest []byte

func main() {
	b := []byte{1, 171, 255}
	fmt.Printf("%x|% x|%X|%v|%d\n", b, b, b, b, b)
	c := []byte("hi")
	c = append(c, 200)
	fmt.Printf("%x %v %s\n", c, c, c[:2])
	fmt.Printf("%#x|% #x|% #X|%.2x|%.1x|%8x|%-8x|\n", b, b, b, b, "abc", b, b)
	fmt.Printf("%#x|% #x|%.0x|%x|\n", "ab", "ab", "ab", "")
	fmt.Printf("%x\n", digest{0xde, 0xad, 0xbe, 0xef})
	fmt.Println(fmt.Sprintf("%02x", []byte{0x0f, 0xf0}))
}
