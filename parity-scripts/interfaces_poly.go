package main

import (
	"fmt"
	"math"
)

type Shape interface {
	area() float64
	name() string
}

type Circle struct{ r float64 }

func (c Circle) area() float64 { return math.Pi * c.r * c.r }
func (c Circle) name() string  { return "circle" }

type Rect struct{ w, h float64 }

func (r Rect) area() float64 { return r.w * r.h }
func (r Rect) name() string  { return "rect" }

func main() {
	shapes := []Shape{Circle{r: 2}, Rect{w: 3, h: 4}, Circle{r: 1}}
	total := 0.0
	for _, s := range shapes {
		fmt.Printf("%s=%.4f\n", s.name(), s.area())
		total += s.area()
	}
	fmt.Printf("total=%.4f\n", total)
}
