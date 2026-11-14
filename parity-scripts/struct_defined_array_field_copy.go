// A struct field whose type is a defined array type is a value: copying the
// struct copies the array, element arrays included.
package main

import "fmt"

type Grid [2][2]int

type Board struct {
	cells Grid
	name  string
}

type Boards struct{ all [2]Grid }

type Row [3]int

type Sheet struct{ rows [2]Row }

func paint(b Board) Board {
	b.cells[0][0] = 9
	return b
}

func main() {
	b1 := Board{name: "x"}
	b2 := b1
	b2.cells[1][1] = 5
	fmt.Println(b1, b2, b1 == b2)

	b3 := paint(b1)
	fmt.Println(b1.cells, b3.cells)

	bs1 := Boards{}
	bs2 := bs1
	bs2.all[1][0][1] = 6
	fmt.Println(bs1, bs2)

	s1 := Sheet{}
	s2 := s1
	s2.rows[0][2] = 4
	fmt.Println(s1, s2)

	m := map[string]Board{"k": {}}
	v := m["k"]
	v.cells[0][1] = 3
	fmt.Println(m["k"].cells, v.cells)

	list := []Board{{name: "a"}}
	w := list[0]
	w.cells[1][0] = 2
	fmt.Println(list[0].cells, w.cells)
}
