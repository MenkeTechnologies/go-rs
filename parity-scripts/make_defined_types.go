// `make` of a defined slice or map type yields a value of that type: it has
// the type's methods and is a map when the type's base is one.
package main

import (
	"fmt"
	"sort"
)

type Item struct{ Price float64 }

type byPrice []*Item

func (b byPrice) Len() int           { return len(b) }
func (b byPrice) Less(i, j int) bool { return b[i].Price < b[j].Price }
func (b byPrice) Swap(i, j int)      { b[i], b[j] = b[j], b[i] }

type Counts map[string]int

func (c Counts) Total() int {
	t := 0
	for _, n := range c {
		t += n
	}
	return t
}

type Ring Counts

func main() {
	list := make(byPrice, 0, 4)
	list = append(list, &Item{3}, &Item{1}, &Item{2})
	sort.Sort(list)
	fmt.Println(list.Len(), list[0].Price, list[2].Price, cap(make(byPrice, 1, 4)))
	c := make(Counts)
	c["a"] += 2
	c["b"]++
	fmt.Println(c, len(c), c.Total())
	r := make(Ring, 2)
	r["x"] = 1
	fmt.Printf("%v %T %T\n", r, r, make(byPrice, 0))
}
