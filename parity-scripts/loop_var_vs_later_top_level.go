// A loop variable and a *later* top-level declaration of the same name are two
// variables: closures and defers made in the loop keep their per-iteration
// values rather than reading the top-level one.
package main

import "fmt"

func main() {
	var fs []func() int
	for i := 0; i < 3; i++ {
		fs = append(fs, func() int { return i })
		defer func(n int) { fmt.Println("defer", n, i) }(i * 10)
	}
	i := 100
	fmt.Println(fs[0](), fs[1](), fs[2](), i)

	for j := range 2 {
		defer func() { fmt.Println("range defer", j) }()
	}
	j := "later"
	fmt.Println(j)
}
