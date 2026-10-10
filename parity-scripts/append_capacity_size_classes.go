// Growth rounds the new array's byte size up to the allocator's size class, so
// the capacity depends on the element's size and on whether it holds pointers
// (a block over 512 bytes of pointers loses 8 bytes to a malloc header). Every
// slice here is printed, which makes it escape and allocate on the heap.
package main

import "fmt"

type P3 struct{ a, b, c int32 }
type PP struct {
	a int8
	p *int
}
type S5 struct{ a, b, c, d, e int }

func g1(n int) {
	var s []int
	var caps []int
	prev := -1
	for i := 0; i < n; i++ {
		s = append(s, 1)
		if cap(s) != prev {
			caps = append(caps, cap(s))
			prev = cap(s)
		}
	}
	fmt.Println("int", caps, len(s))
	fmt.Println(s[:0])
}

func g2(n int) {
	var s []int8
	var caps []int
	prev := -1
	for i := 0; i < n; i++ {
		s = append(s, 1)
		if cap(s) != prev {
			caps = append(caps, cap(s))
			prev = cap(s)
		}
	}
	fmt.Println("int8", caps, len(s))
	fmt.Println(s[:0])
}

func g3(n int) {
	var s []int16
	var caps []int
	prev := -1
	for i := 0; i < n; i++ {
		s = append(s, 1)
		if cap(s) != prev {
			caps = append(caps, cap(s))
			prev = cap(s)
		}
	}
	fmt.Println("int16", caps, len(s))
	fmt.Println(s[:0])
}

func g4(n int) {
	var s []int32
	var caps []int
	prev := -1
	for i := 0; i < n; i++ {
		s = append(s, 1)
		if cap(s) != prev {
			caps = append(caps, cap(s))
			prev = cap(s)
		}
	}
	fmt.Println("int32", caps, len(s))
	fmt.Println(s[:0])
}

func g5(n int) {
	var s []float64
	var caps []int
	prev := -1
	for i := 0; i < n; i++ {
		s = append(s, 1.5)
		if cap(s) != prev {
			caps = append(caps, cap(s))
			prev = cap(s)
		}
	}
	fmt.Println("float64", caps, len(s))
	fmt.Println(s[:0])
}

func g6(n int) {
	var s []bool
	var caps []int
	prev := -1
	for i := 0; i < n; i++ {
		s = append(s, true)
		if cap(s) != prev {
			caps = append(caps, cap(s))
			prev = cap(s)
		}
	}
	fmt.Println("bool", caps, len(s))
	fmt.Println(s[:0])
}

func g7(n int) {
	var s []string
	var caps []int
	prev := -1
	for i := 0; i < n; i++ {
		s = append(s, "s")
		if cap(s) != prev {
			caps = append(caps, cap(s))
			prev = cap(s)
		}
	}
	fmt.Println("string", caps, len(s))
	fmt.Println(s[:0])
}

func g8(n int) {
	var s []P3
	var caps []int
	prev := -1
	for i := 0; i < n; i++ {
		s = append(s, P3{})
		if cap(s) != prev {
			caps = append(caps, cap(s))
			prev = cap(s)
		}
	}
	fmt.Println("P3", caps, len(s))
	fmt.Println(s[:0])
}

func g9(n int) {
	var s []PP
	var caps []int
	prev := -1
	for i := 0; i < n; i++ {
		s = append(s, PP{})
		if cap(s) != prev {
			caps = append(caps, cap(s))
			prev = cap(s)
		}
	}
	fmt.Println("PP", caps, len(s))
	fmt.Println(s[:0])
}

func g10(n int) {
	var s []S5
	var caps []int
	prev := -1
	for i := 0; i < n; i++ {
		s = append(s, S5{})
		if cap(s) != prev {
			caps = append(caps, cap(s))
			prev = cap(s)
		}
	}
	fmt.Println("S5", caps, len(s))
	fmt.Println(s[:0])
}

func g11(n int) {
	var s [][3]int
	var caps []int
	prev := -1
	for i := 0; i < n; i++ {
		s = append(s, [3]int{})
		if cap(s) != prev {
			caps = append(caps, cap(s))
			prev = cap(s)
		}
	}
	fmt.Println("[3]int", caps, len(s))
	fmt.Println(s[:0])
}

func g12(n int) {
	var s [][]int
	var caps []int
	prev := -1
	for i := 0; i < n; i++ {
		s = append(s, []int(nil))
		if cap(s) != prev {
			caps = append(caps, cap(s))
			prev = cap(s)
		}
	}
	fmt.Println("[]int", caps, len(s))
	fmt.Println(s[:0])
}

func g13(n int) {
	var s []*int
	var caps []int
	prev := -1
	for i := 0; i < n; i++ {
		s = append(s, (*int)(nil))
		if cap(s) != prev {
			caps = append(caps, cap(s))
			prev = cap(s)
		}
	}
	fmt.Println("*int", caps, len(s))
	fmt.Println(s[:0])
}

func g14(n int) {
	var s []byte
	var caps []int
	prev := -1
	for i := 0; i < n; i++ {
		s = append(s, 1)
		if cap(s) != prev {
			caps = append(caps, cap(s))
			prev = cap(s)
		}
	}
	fmt.Println("byte", caps, len(s))
	fmt.Println(s[:0])
}

func g15(n int) {
	var s []int64
	var caps []int
	prev := -1
	for i := 0; i < n; i++ {
		s = append(s, 1)
		if cap(s) != prev {
			caps = append(caps, cap(s))
			prev = cap(s)
		}
	}
	fmt.Println("int64", caps, len(s))
	fmt.Println(s[:0])
}

func g16(n int) {
	var s []uint16
	var caps []int
	prev := -1
	for i := 0; i < n; i++ {
		s = append(s, 1)
		if cap(s) != prev {
			caps = append(caps, cap(s))
			prev = cap(s)
		}
	}
	fmt.Println("uint16", caps, len(s))
	fmt.Println(s[:0])
}

func g17(n int) {
	var s []complex128
	var caps []int
	prev := -1
	for i := 0; i < n; i++ {
		s = append(s, 1)
		if cap(s) != prev {
			caps = append(caps, cap(s))
			prev = cap(s)
		}
	}
	fmt.Println("complex128", caps, len(s))
	fmt.Println(s[:0])
}

func g18(n int) {
	var s []any
	var caps []int
	prev := -1
	for i := 0; i < n; i++ {
		s = append(s, 1)
		if cap(s) != prev {
			caps = append(caps, cap(s))
			prev = cap(s)
		}
	}
	fmt.Println("any", caps, len(s))
	fmt.Println(s[:0])
}

func g19(n int) {
	var s []map[string]int
	var caps []int
	prev := -1
	for i := 0; i < n; i++ {
		s = append(s, map[string]int(nil))
		if cap(s) != prev {
			caps = append(caps, cap(s))
			prev = cap(s)
		}
	}
	fmt.Println("map[string]int", caps, len(s))
	fmt.Println(s[:0])
}

func g20(n int) {
	var s []error
	var caps []int
	prev := -1
	for i := 0; i < n; i++ {
		s = append(s, error(nil))
		if cap(s) != prev {
			caps = append(caps, cap(s))
			prev = cap(s)
		}
	}
	fmt.Println("error", caps, len(s))
	fmt.Println(s[:0])
}

func main() {
	g1(700)
	g2(700)
	g3(4000)
	g4(700)
	g5(700)
	g6(4000)
	g7(700)
	g8(700)
	g9(4000)
	g10(700)
	g11(700)
	g12(4000)
	g13(700)
	g14(700)
	g15(4000)
	g16(700)
	g17(700)
	g18(4000)
	g19(700)
	g20(700)
}
