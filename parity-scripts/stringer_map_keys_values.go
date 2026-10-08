package main

import (
	"errors"
	"fmt"
)

type Weekday int

func (d Weekday) String() string {
	return [...]string{"Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"}[d]
}

type K struct{ n int }

func (k K) String() string { return fmt.Sprint("K", k.n) }

type P struct{ X, Y int }

func (p *P) String() string { return "ptr" }

type Same int

func (Same) String() string { return "same" }

func main() {
	fmt.Println(map[string]Weekday{"a": 1, "b": 3})
	fmt.Println(map[Weekday]int{6: 2, 0: 5, 3: 1})
	fmt.Println(map[K]string{{2}: "x", {1}: "y"})
	fmt.Printf("%v|%s|%d|%+v\n", map[Weekday]Weekday{1: 2}, map[Weekday]Weekday{1: 2}, map[Weekday]Weekday{1: 2}, map[Weekday]int{4: 4})
	fmt.Printf("%T\n", map[Weekday]int{})
	var nm map[Weekday]int
	fmt.Println(nm, len(nm))
	fmt.Println(map[string]error{"e": errors.New("boom")})
	fmt.Println(map[int]P{1: {1, 2}}, map[int]*P{1: {1, 2}})
	fmt.Println(map[Same]int{1: 1, 2: 2})
	m := map[int]Weekday{10: 1, 9: 2}
	fmt.Println(m, len(m))
	s := fmt.Sprint(map[Weekday]bool{2: true})
	fmt.Println(s)
}
