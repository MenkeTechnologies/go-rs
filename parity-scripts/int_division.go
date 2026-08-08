package main

// Go's `/` is integer division whenever both operands are integers, including
// when the operand's type is only known at run time (a slice element, a map
// value, a struct field reached through an interface). go-rs used to fall back
// to float division for those, so `xs[0] / 2` produced 3.5 instead of 3.
import "fmt"

type box struct{ n int }

func half(v int) int { return v / 2 }

func main() {
	xs := []int{7, -7, 0, 21}
	ys := []int{2, 2, 3, -4}
	for i := range xs {
		fmt.Println(xs[i]/ys[i], xs[i]%ys[i])
	}

	m := map[string]int{"a": 9, "b": -9}
	fmt.Println(m["a"]/2, m["b"]/2, m["a"]%2, m["b"]%2)

	b := box{7}
	fmt.Println(b.n/2, half(b.n))

	// Constant folding and statically-typed variables take the same path.
	a, c := 7, 2
	fmt.Println(a/c, 7/2, -7/2, 7/-2, -7/-2)

	// Float division stays float when either side is a float.
	f := 7.0
	fmt.Println(f/2, 7.0/2, float64(a)/2)

	// A float-typed composite literal converts its integer constants, so its
	// elements divide as floats.
	fs := []float64{7, 1}
	fmt.Println(fs[0]/2, fs[1]/4)
	fm := map[string]float64{"k": 7}
	fmt.Println(fm["k"] / 2)
}
