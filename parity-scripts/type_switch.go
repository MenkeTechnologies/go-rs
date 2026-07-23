// Type switches and type assertions over the empty interface (`any`).
package main

import "fmt"

type Circle struct{ r int }
type Square struct{ side int }

func describe(x any) string {
	switch v := x.(type) {
	case int:
		return fmt.Sprintf("int %d", v*2)
	case string:
		return "str " + v
	case bool:
		return fmt.Sprintf("bool %t", v)
	case float64:
		return fmt.Sprintf("float %.2f", v)
	case Circle:
		return fmt.Sprintf("circle r=%d", v.r)
	case Square:
		return fmt.Sprintf("square s=%d", v.side)
	default:
		return "?"
	}
}

func main() {
	for _, x := range []any{7, "hi", true, 2.5, Circle{r: 3}, Square{side: 4}, []int{1}} {
		fmt.Println(describe(x))
	}

	// comma-ok assertions
	var i any = "hello"
	if s, ok := i.(string); ok {
		fmt.Println("got string:", s)
	}
	n, ok := i.(int)
	fmt.Println(n, ok)

	// single assertion
	var j any = 41
	fmt.Println(j.(int) + 1)
}
