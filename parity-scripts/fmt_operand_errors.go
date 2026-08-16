package main

// The `fmt` forms a malformed call reaches: too few operands, too many, a `%`
// that never reaches a verb character, and a verb that is not one. A formatter
// that substitutes a zero value for a missing operand reports a malformed call
// as a well-formed one, so these are the cases nothing else catches.

import "fmt"

func main() {
	fmt.Println(fmt.Sprintf("%d %s", 1))
	fmt.Println(fmt.Sprintf("%d", 1, 2))
	fmt.Println(fmt.Sprintf("%d", 1, 2, "x"))
	fmt.Println(fmt.Sprintf("%z", 1))
	fmt.Println(fmt.Sprintf("%!"))
	fmt.Println(fmt.Sprintf("%"))
	fmt.Println(fmt.Sprintf("abc%"))
	fmt.Println(fmt.Sprintf("%d%", 1))
	fmt.Println(fmt.Sprintf("%v", nil))
	fmt.Println(fmt.Sprintf("%s"))
	fmt.Println(fmt.Sprintf("%q"))
	fmt.Println(fmt.Sprintf("%f"))
	fmt.Println(fmt.Sprintf("%d %d %d", 1))
	fmt.Println(fmt.Sprintf("no verbs", 1))
	fmt.Println(fmt.Sprintf("no verbs", 1, "two"))
	fmt.Println(fmt.Sprintf("%y %d", 1, 2))
	fmt.Println(fmt.Sprintf("%5.2z", 3))
	fmt.Println(fmt.Sprintf("%T"))
	fmt.Println(fmt.Sprintf("%%d", 1))
	fmt.Println(fmt.Sprintf("%v", []int{1}, "extra"))
	fmt.Println(fmt.Sprintf("%s", true))
}
