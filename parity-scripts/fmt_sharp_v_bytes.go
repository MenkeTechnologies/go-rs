// %#v writes each byte of a byte slice or array as `%#x` with no padding
// (`0x1`), an array under its array type and a slice as []byte at depth 0.
package main

import "fmt"

func main() {
	b := []byte{1, 2, 255}
	var a [3]byte
	a[0] = 9
	fmt.Printf("%#v\n%#v\n%v %x\n", b, a, a, a)
	fmt.Printf("%#v %#v\n", [2]uint8{1, 2}, [0]int{})
	fmt.Printf("%#v %#v\n", [2][]byte{{1}, {2}}, struct{ B [2]byte }{[2]byte{3, 4}})
}
