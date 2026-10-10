// os.Exit used as a function value ends the process without running defers.
package main

import (
	"fmt"
	"os"
)

func run(f func(int), code int) { f(code) }

func main() {
	fmt.Println("before")
	exit := os.Exit
	defer fmt.Println("deferred calls do not run")
	run(exit, 3)
	fmt.Println("unreachable")
}
