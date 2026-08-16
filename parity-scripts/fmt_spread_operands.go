package main

// `fmt.Printf(f, args...)` — the spread slice stands for the operands it holds,
// not for one operand of its own. This is the shape every logging wrapper in Go
// is written in, and formatting the slice instead silently prints `[1 2]` where
// Go prints `1 2`.

import "fmt"

func logf(f string, a ...any) {
	fmt.Printf(f, a...)
}

func joined(a ...any) string {
	return fmt.Sprint(a...)
}

func lined(a ...any) string {
	return fmt.Sprintln(a...)
}

func wrapped(f string, a ...any) error {
	return fmt.Errorf("wrapped: "+f, a...)
}

func main() {
	logf("%d-%d\n", 1, 2)
	logf("none\n")
	logf("%s=%v %T\n", "k", 3.5, 7)

	fmt.Println(joined())
	fmt.Println(joined(1))
	fmt.Println(joined(1, 2, "x"))
	fmt.Print(lined(1, 2))

	fmt.Println(fmt.Sprintf("%d/%d", 3, 4))

	// A spread through two hops still arrives as operands.
	fmt.Println(wrapped("%d", 9))

	// An empty spread leaves the format with no operands at all.
	var none []any
	fmt.Printf("%s\n", fmt.Sprint(none...))

	// A spread of a built slice, and the non-spread call beside it.
	xs := []any{"a", 1}
	fmt.Println(fmt.Sprint(xs...))
	fmt.Println(fmt.Sprint(xs))
}
