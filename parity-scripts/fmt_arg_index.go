// `%[n]` — fmt's explicit operand index, the BADINDEX forms it rejects, and the
// `%!(EXTRA …)` report an index suppresses.
package main

import "fmt"

func main() {
	fmt.Printf("[%[1]d][%[2]s][%[1]v]\n", 3, "b")
	fmt.Printf("[%[2]d %[1]d %[1]s]\n", "x", 7)
	fmt.Printf("[%[1]*d][%[2]*[1]d]\n", 3, 4)
	fmt.Printf("[%.[2]d]\n", 5, 3)
	fmt.Printf("[%[3]d][%[0]d]\n", 1, 2)
	fmt.Printf("[%[x]d][%[]d]\n", 1)
	fmt.Printf("[%[1]2d][%[1].2d]\n", 5)
	fmt.Printf("[%[1]d]\n", 1, 2, 3)
	fmt.Printf("[%d]\n", 1, 2)
	fmt.Printf("[%[2]v %[1]v %v]\n", "a", "b", "c")
}
