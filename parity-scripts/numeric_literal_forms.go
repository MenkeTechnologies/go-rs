package main

// Integer and float literal spellings: hexadecimal floating point (`0x1.8p1`,
// a fraction-only mantissa, `_` separators, an exponent pushing into the
// subnormals, a mantissa with more digits than a float64 holds), and the
// legacy `0`-prefixed octal integer beside `0o`, which a decimal point turns
// back into a decimal float.

import "fmt"

func main() {
	fmt.Println(0x1.fffp-3, 0x1p-2, 0x.8p1, 0X1P+10, 0x1_0.8p0, 0x1p-1074, 0x1.fffffffffffff8p0, 0x1.fffffffffffff7ffp0)
	fmt.Println(0755, 0o755, 00, 0_7, 017+1, 0.5, 01.5, 09.5, 0e3)
	x := 0x1p4
	fmt.Printf("%T %v\n", x, x)
	const big = 0777777
	fmt.Println(big, big>>3)
}
