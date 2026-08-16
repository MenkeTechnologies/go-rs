package main

// `%T` of every sized integer type. All of them are one `Value::Int` at run
// time, so the width survives only in the static type — which is why the
// compiler tags a `fmt` operand with it. Without the tag every line here
// answers `int`.

import "fmt"

type myByte byte
type myCount int32

func main() {
	var i8 int8 = 5
	var i16 int16 = 5
	var i32 int32 = 5
	var i64 int64 = 5
	var u8 uint8 = 5
	var u16 uint16 = 5
	var u32 uint32 = 5
	var u64 uint64 = 5
	var u uint = 5
	var f32 float32 = 5
	var f64 float64 = 5
	var b byte = 5
	var r rune = 5

	fmt.Printf("%T %T %T %T\n", i8, i16, i32, i64)
	fmt.Printf("%T %T %T %T %T\n", u8, u16, u32, u64, u)
	fmt.Printf("%T %T %T %T\n", f32, f64, b, r)

	// `byte` and `rune` are aliases: `%T` prints the type they name.
	fmt.Printf("%T %T\n", []byte("ab"), []rune("ab"))
	fmt.Printf("%T %T\n", []byte("ab")[0], []rune("ab")[0])

	// Arithmetic keeps the width; a conversion introduces it.
	fmt.Printf("%T %T %T\n", int8(1)+int8(2), i32*2, -i16)
	fmt.Printf("%T %T\n", i8<<1, uint16(3)|uint16(4))

	// Containers name their element type, and an indexed read has it.
	xs := []int8{1, 2}
	fmt.Printf("%T %T\n", xs, xs[0])
	m := map[string]uint8{"a": 1}
	fmt.Printf("%T %T\n", m, m["a"])
	arr := [2]int16{1, 2}
	fmt.Printf("%T %T\n", arr, arr[0])

	// A defined type outranks its base width: it is named, not described.
	var mb myByte = 7
	var mc myCount = 7
	fmt.Printf("%T %T\n", mb, mc)
	fmt.Printf("%T\n", map[myByte]myCount{1: 2})

	// Every other verb sees straight through the tag to the value.
	fmt.Println(i8, u8, f32, b, r, mb, mc)
	fmt.Printf("%v %d %v %d\n", i8, u64, mb, mc)
}
