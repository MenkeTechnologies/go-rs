package main

// Integer `/` truncates toward zero and `%` takes the sign of the dividend. Both
// matter because the VM's division op is float-biased: `7/2` is 3, not 3.5, and
// a frontend that lowers to the float op silently returns the wrong type.

import "fmt"

func main() {
	fmt.Println(7/2, -7/2, 7/-2, -7/-2)
	fmt.Println(7%2, -7%2, 7%-2, -7%-2)
	a, b := 9, 4
	fmt.Println(a/b, a%b, -a/b, -a%b)
	fmt.Println(1/3, 2/3, 5/5, 0/7)
	fmt.Printf("%v %T\n", 7/2, 7/2)
	var x int64 = 100
	var y int64 = 7
	fmt.Println(x/y, x%y)
	fmt.Println(float64(7)/2, 7.0/2)
	c := 7
	c /= 2
	fmt.Println(c)
	d := -7
	d %= 3
	fmt.Println(d)
}
