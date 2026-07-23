package main

import "fmt"

type Shape interface {
	area() int
	name() string
}

type Rect struct {
	w int
	h int
}

func (r Rect) area() int {
	return r.w * r.h
}
func (r Rect) name() string {
	return "rect"
}

type Square struct {
	s int
}

func (sq Square) area() int {
	return sq.s * sq.s
}
func (sq Square) name() string {
	return "square"
}

func describe(s Shape) {
	fmt.Println(s.name(), s.area())
}

func main() {
	describe(Rect{w: 3, h: 4})
	describe(Square{s: 5})

	shapes := []Shape{Rect{w: 2, h: 2}, Square{s: 3}}
	total := 0
	for _, s := range shapes {
		total += s.area()
	}
	fmt.Println("total", total)
}
