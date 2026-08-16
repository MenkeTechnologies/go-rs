package main

// `*` takes a width or a precision from an operand instead of from the format,
// so it changes how many operands the verbs after it consume. A negative width
// means left-alignment, and a non-`int` there is reported without losing the
// verb.

import "fmt"

func main() {
	fmt.Println(fmt.Sprintf("%*d", 6, 42))
	fmt.Println(fmt.Sprintf("%-*d|", 6, 42))
	fmt.Println(fmt.Sprintf("%.*f", 2, 3.14159))
	fmt.Println(fmt.Sprintf("%*.*f", 9, 2, 3.14159))
	fmt.Println(fmt.Sprintf("%*d", -6, 42))
	fmt.Println(fmt.Sprintf("%*d", 6))
	fmt.Println(fmt.Sprintf("%.*f", 2))
	fmt.Println(fmt.Sprintf("%*d", "x", 42))
	fmt.Println(fmt.Sprintf("%.*f", "x", 1.5))
	fmt.Println(fmt.Sprintf("%0*d", 5, 42))
	fmt.Println(fmt.Sprintf("%*s", 5, "ab"))
}
