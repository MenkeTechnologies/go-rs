// The `0` flag fills every verb `fmt` lets it reach — not only the numeric ones
// — and a width on `%v` lands on each element of a composite, not on the list.
package main

import "fmt"

type zfS struct {
	A int
	B string
}

func main() {
	fmt.Printf("[%010s][%010q][%010t][%010c]\n", "z", "z", true, 'A')
	fmt.Printf("[%010U][%010T][%08.3g][%010e]\n", 'A', 5, 1.5, 1.5)
	fmt.Printf("[%05.2s][%05.2q]\n", "abcdef", "abcdef")
	fmt.Printf("[%010x][%010X]\n", "ab", []byte("ab"))
	fmt.Printf("[%-010s][%-010d]\n", "z", 5)
	fmt.Printf("[%+010d][% 010d][%#010x]\n", 5, 5, -9)
	fmt.Printf("[%10v]\n", []int{1, 2})
	fmt.Printf("[%010v]\n", []int{1, 2})
	fmt.Printf("[%10v]\n", zfS{1, "x"})
	fmt.Printf("[%010v]\n", map[string]int{"a": 1})
	fmt.Printf("[%010s]\n", []string{"a"})
	var nilS []int
	fmt.Printf("[%010v][%010v][%010v]\n", nilS, nil, true)
	fmt.Printf("[%p][%y][%y]\n", nil, nil, 5)
}
