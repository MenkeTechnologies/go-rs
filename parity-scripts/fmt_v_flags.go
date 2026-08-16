package main

// `%v` under each flag. An integer takes the `%d` treatment — the digit-count
// precision and the zero fill — while `+` and `#` are not number flags and must
// not divert it.

import "fmt"

func main() {
	fmt.Println(fmt.Sprintf("%+v", 42))
	fmt.Println(fmt.Sprintf("%05v", 42))
	fmt.Println(fmt.Sprintf("%#v", 42))
	fmt.Println(fmt.Sprintf("% d", 42))
	fmt.Println(fmt.Sprintf("% d", -42))
	fmt.Println(fmt.Sprintf("%5v", 42))
	fmt.Println(fmt.Sprintf("%-5v|", 42))
	fmt.Println(fmt.Sprintf("%.2v", 12345))
	fmt.Println(fmt.Sprintf("%.2v", -3))
	fmt.Println(fmt.Sprintf("%05v", "ab"))
	fmt.Println(fmt.Sprintf("%05.1f", 3.5))
	fmt.Println(fmt.Sprintf("%T", 1))
}
