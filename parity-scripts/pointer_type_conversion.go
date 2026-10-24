package main

import (
	"fmt"
	"io"
)

// `(*T)(p)` converts a pointer: the value is the same, the type is `*T`.

type stringReader string

func (r *stringReader) Read(b []byte) (int, error) {
	n := copy(b, *r)
	*r = (*r)[n:]
	if n == 0 {
		return 0, io.EOF
	}
	return n, nil
}

type Celsius float64

func (c Celsius) String() string { return fmt.Sprintf("%.1fC", float64(c)) }

func main() {
	str := "hello"
	var rd io.Reader = (*stringReader)(&str)
	b := make([]byte, 3)
	for {
		n, err := rd.Read(b)
		fmt.Printf("%d %v %q %q\n", n, err, b[:n], str)
		if err != nil {
			break
		}
	}
	fmt.Printf("%T\n", rd)

	f := 21.5
	pc := (*Celsius)(&f)
	fmt.Println(*pc, pc.String())
	*pc = 30
	fmt.Println(f)
	fmt.Printf("%T %T\n", pc, any(pc))
	var s fmt.Stringer = pc
	fmt.Println(s)

	x := 7
	px := (*int)(&x)
	*px = 8
	fmt.Println(x)

	// fmt.Sscan reads its string through the same conversion.
	var w1, w2 string
	n, err := fmt.Sscan("  alpha\n beta ", &w1, &w2)
	fmt.Println(n, err, w1, w2)
}
