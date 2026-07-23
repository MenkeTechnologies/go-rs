// Bitwise operators, base-prefixed / underscored integer literals, type
// conversions, and const blocks with iota (including a defined type).
package main

import "fmt"

type Flag int

const (
	Read Flag = 1 << iota
	Write
	Exec
)

const (
	_  = iota
	KB = 1 << (10 * iota)
	MB
	GB
)

// A rolling checksum masked to 16 bits each step (stays in range, so no
// dependence on fixed-width integer overflow).
func checksum(s string) int {
	h := 0
	for _, c := range s {
		h = ((h << 5) ^ h ^ int(c)) & 0xFFFF
	}
	return h
}

func main() {
	a, b := 0b1100, 0b1010
	fmt.Println(a&b, a|b, a^b, a&^b, ^a&0xFF)
	fmt.Println(1<<8, 0xFF00>>8, 0o755)
	fmt.Println(1_000_000, 3.141_592)

	// compound bitwise assignment (flag building)
	perm := Read
	perm |= Write
	perm &^= Read
	fmt.Println(perm, perm&Write != 0, perm&Exec != 0)
	fmt.Println(KB, MB, GB)

	f := 3.99
	fmt.Println(int(f), float64(7)/2, string(rune(0x41)))
	fmt.Println(checksum("go-rs"))
}
