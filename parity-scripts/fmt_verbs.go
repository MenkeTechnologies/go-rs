package main

// The `fmt` verbs beyond %v/%d/%s: the struct forms %+v and %#v, the type verb
// %T, Go-syntax quoting for %q, and %x/%U/%c on text and code points.
import "fmt"

type point struct {
	X int
	Y string
	Z float64
}

type wrapper struct {
	P    point
	Tags []string
}

func main() {
	p := point{1, "a", 2.5}
	fmt.Printf("%v\n%+v\n%#v\n%T\n", p, p, p, p)

	w := wrapper{p, []string{"x", "y"}}
	fmt.Printf("%v\n%+v\n", w, w)

	fmt.Printf("%T %T %T %T %T\n", 1, "s", 1.5, true, p)
	fmt.Printf("%T %T\n", []int{1}, map[string]int{"a": 1})

	// %q quotes strings with Go escapes and integers as rune literals.
	fmt.Printf("%q\n", "tab\there\nnew \"quoted\" \\ back")
	fmt.Printf("%q %q %q\n", 'A', '\n', '世')
	fmt.Printf("%q\n", "héllo 世界")

	// %x/%X hex-encode a string's bytes; on an integer they are base-16 digits.
	fmt.Printf("%x %X %x %X\n", "ab", "ab", 255, 255)
	fmt.Printf("%x %X\n", []int{10, 11}, []int{10, 11})
	fmt.Printf("%#x %#X\n", 255, 255)

	// %U, %c and %d on a rune.
	fmt.Printf("%U %c %d\n", '世', '世', '世')
	fmt.Printf("%U %U\n", 'A', 0x10FFFF)

	// %d distributes over a slice.
	fmt.Printf("%d %d\n", []int{1, 2, 3}, 42)

	// %s on a string, and precision truncation.
	fmt.Printf("%s|%.2s|%6s|%-6s|\n", "hello", "hello", "hi", "hi")

	// %o, %b, %t and zero/width padding.
	fmt.Printf("%o %b %t %05d %5.2f\n", 64, 5, true, 42, 3.14159)
	fmt.Println(fmt.Sprintf("%05d|%x|%X|%o|%b", 42, 255, 255, 8, 5))
}
