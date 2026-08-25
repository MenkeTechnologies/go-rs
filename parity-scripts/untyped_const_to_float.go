package main

// Go converts an untyped constant to the destination's type at every binding
// site — a call argument, a channel send, a container element, a struct field.
// go-rs represents an `int` and a `float64` differently at run time, so a `1`
// that never made that conversion stays an integer that only the *static* type
// calls a float. Arithmetic reads the static type and so never noticed; `%T`
// and a map key read the value, and did.
//
// Every line below is a place a float destination takes an untyped constant.

import "fmt"

type box struct {
	F float64
	G float32
}

type celsius float64

func (b *box) set(v float64)        { b.F = v }
func (b box) kind(v float64) string { return fmt.Sprintf("%T", v) }

func plain(v float64) string { return fmt.Sprintf("%T %v", v, v/2) }
func vari(first float64, rest ...float64) string {
	return fmt.Sprintf("%T %d", first, len(rest))
}
func ret() float64 { return 1 }

func main() {
	// A call argument, a method argument, a closure argument, a variadic one.
	fmt.Println(plain(1))
	b := &box{}
	b.set(1)
	fmt.Println(b.F, b.F/2, b.kind(1))
	lam := func(v float64) string { return fmt.Sprintf("%T %v", v, v/4) }
	fmt.Println(lam(1))
	fmt.Println(vari(1, 2, 3))

	// A return value, a `var` with a written type, a struct literal field.
	fmt.Printf("%T %v\n", ret(), ret()/2)
	var v float64 = 1
	fmt.Printf("%T %v\n", v, v/2)
	fmt.Printf("%T %v\n", box{1, 2}.F, box{1, 2}.F/2)

	// A struct field written after the fact.
	var c box
	c.F = 1
	fmt.Printf("%T %v\n", c.F, c.F/2)

	// A slice element, an array element, a map value — literal and assigned.
	s := []float64{1, 2}
	s[0] = 3
	fmt.Printf("%T %v %v\n", s[0], s[0]/2, s[1]/4)
	var arr [2]float64
	arr[0] = 1
	fmt.Printf("%T %v\n", arr[0], arr[0]/2)
	m := map[string]float64{"a": 1}
	m["b"] = 3
	fmt.Printf("%T %v %v\n", m["a"], m["a"]/2, m["b"]/2)

	// `append`, and a channel send.
	ap := append([]float64{}, 1)
	fmt.Printf("%T %v\n", ap[0], ap[0]/2)
	ch := make(chan float64, 1)
	ch <- 1
	got := <-ch
	fmt.Printf("%T %v\n", got, got/2)

	// A defined float type takes the same conversions.
	var t celsius = 1
	tm := map[celsius]string{1: "one", 2.5: "two-five"}
	fmt.Println(t/2, tm[1], tm[celsius(1)], tm[t], tm[2.5])

	// A float-keyed map reached from each of those, which is where an integer
	// wearing a float's static type stops being invisible.
	fk := map[float64]string{1: "one", 3: "three"}
	fmt.Println(fk[b.F], fk[c.F], fk[ret()], fk[v], fk[ap[0]], fk[s[0]], fk[arr[0]], fk[got], fk[m["b"]])

	// `delete` looks the key up the same way.
	delete(fk, 1)
	_, still := fk[1]
	fmt.Println(len(fk), still, fk[3])

	// float32 keys and values keep their own width through all of it.
	f32 := map[float32]string{1: "a"}
	fmt.Println(f32[1], f32[float32(1)], box{1, 2}.G/4)

	// An `int`-keyed map is untouched by any of this.
	im := map[int]string{1: "one"}
	fmt.Println(im[1], im[int(1)], len(im))
}
