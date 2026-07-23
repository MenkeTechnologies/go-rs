package main

import "fmt"

func main() {
	fmt.Println(2+3*4, (2+3)*4, 17/5, 17%5, 17.0/5.0, -7/2, -7%2)
	fmt.Println(1 < 2, 2 <= 2, 3 > 4, 5 != 5, true && false, true || false, !true)
}
