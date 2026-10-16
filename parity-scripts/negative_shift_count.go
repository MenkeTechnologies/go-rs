package main

import "fmt"

func try(name string, f func()) {
	defer func() { fmt.Println(name, recover()) }()
	f()
}

func main() {
	n := -1
	try("shl", func() { fmt.Println(1 << n) })
	try("shr", func() { fmt.Println(8 >> n) })
	try("shl-assign", func() { x := 4; x <<= n; fmt.Println(x) })
	try("shr-assign", func() { x := 4; x >>= n; fmt.Println(x) })
	try("neg-expr", func() { k := 2; fmt.Println(1 << (k - 3)) })
	// non-negative counts, including at and past the width, still shift
	for _, c := range []int{0, 1, 63, 64, 100} {
		fmt.Println(c, 1<<c, -8>>c, uint(5)<<uint(c))
	}
	var u uint = 70
	fmt.Println(3<<u, 1<<(u-69))
}
