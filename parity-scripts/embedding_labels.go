package main

import "fmt"

// Embedded fields and their promotion, elided composite-literal element types,
// and labeled break/continue — checked byte-for-byte against the real `go`.

type Base struct {
	N    int
	Name string
}

func (b Base) Describe() string { return fmt.Sprintf("Base(%d,%s)", b.N, b.Name) }
func (b Base) Double() int      { return b.N * 2 }
func (b *Base) Bump()           { b.N += 10 }

type Middle struct {
	Base
	Tag string
}

func (m Middle) Describe() string { return "Middle:" + m.Tag }

type Derived struct {
	Middle
	Extra int
}

type Describer interface{ Describe() string }

type Animal struct{ Legs int }

func (a *Animal) AddLeg()     { a.Legs++ }
func (a Animal) Walk() string { return fmt.Sprintf("walking on %d", a.Legs) }

type Dog struct {
	*Animal
	Name string
}

type Point struct{ X, Y int }

func main() {
	d := Derived{Middle: Middle{Base: Base{N: 3, Name: "x"}, Tag: "t"}, Extra: 9}
	fmt.Println(d.N, d.Name, d.Tag, d.Extra, d)
	fmt.Println(d.Describe(), d.Double(), d.Base.Describe(), d.Middle.Base.N)
	d.N = 42
	fmt.Println(d.N, d.Base.N, d.Double())
	d.Bump()
	fmt.Println(d.N, d.Base.N)

	for _, i := range []Describer{d, d.Middle, d.Base} {
		fmt.Println(i.Describe())
	}

	dog := Dog{Animal: &Animal{Legs: 4}, Name: "rex"}
	dog.AddLeg()
	fmt.Println(dog.Legs, dog.Animal.Legs, dog.Name, dog.Walk())

	fmt.Println([][]int{{1, 2}, {3, 4, 5}, {}})
	fmt.Println([]Point{{1, 2}, {X: 3}, {}})
	fmt.Println(map[string][]int{"a": {1, 2}, "b": {3}})
	fmt.Println(map[string]map[string]int{"x": {"i": 1}, "y": {}})
	fmt.Println([2][2]int{{1, 2}, {3, 4}}, [4][]int{2: {7, 8}})
	fmt.Println([][][]int{{{1}, {2, 3}}, {{4}}})
	fmt.Println(map[Point]string{{1, 2}: "a"}[Point{1, 2}])

outer:
	for i := 0; i < 4; i++ {
		for j := 0; j < 4; j++ {
			if j == 2 {
				continue outer
			}
			if i == 3 {
				break outer
			}
			fmt.Println(i, j)
		}
	}

loop:
	for i := range 5 {
		switch i {
		case 2:
			continue loop
		case 4:
			break loop
		}
		fmt.Println("i =", i)
	}

	n := 0
sw:
	switch n {
	case 0:
		for k := 0; k < 3; k++ {
			if k == 1 {
				break sw
			}
			fmt.Println("k", k)
		}
		fmt.Println("unreachable")
	}
	fmt.Println("done")
}
