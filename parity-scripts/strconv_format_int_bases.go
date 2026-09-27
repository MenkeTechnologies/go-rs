package main

// strconv.FormatInt writes a sign and the magnitude in any base 2..36, and
// panics on any other base.

import (
	"fmt"
	"strconv"
)

func main() {
	for _, b := range []int{2, 8, 10, 16, 36, 7} {
		fmt.Println(strconv.FormatInt(-255, b), strconv.FormatInt(255, b), strconv.FormatInt(0, b))
	}
	fmt.Println(strconv.FormatInt(-9223372036854775808, 16), strconv.FormatInt(9223372036854775807, 36))
	defer func() { fmt.Println(recover()) }()
	fmt.Println(strconv.FormatInt(5, 1))
}
