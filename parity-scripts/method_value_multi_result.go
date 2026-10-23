// A method with several results, called through a *T variable that came from
// a function's result, and taken as a method value: each call destructures
// every result, and the method value has the method's whole signature.
package main

import (
	"errors"
	"fmt"
)

type Counter struct{ n int }

func (c *Counter) Next(step int) (int, error) {
	if step < 0 {
		return c.n, errors.New("negative step")
	}
	c.n += step
	return c.n, nil
}

func (c Counter) Pair() (int, string) { return c.n, fmt.Sprint("n=", c.n) }

func newCounter() (*Counter, error) { return &Counter{n: 10}, nil }

func apply(f func(int) (int, error), steps ...int) {
	for _, s := range steps {
		v, err := f(s)
		fmt.Println(v, err)
	}
}

func main() {
	c, err := newCounter()
	v, err := c.Next(5)
	fmt.Println(v, err)

	next := c.Next
	apply(next, 1, -1, 2)
	a, b := next(0)
	fmt.Println(a, b)
	fmt.Println(next(3))

	pair := c.Pair
	n, s := pair()
	fmt.Println(n, s)
	c.n = 100 // a value-receiver method value bound the copy taken above
	fmt.Println(pair())
	fmt.Printf("%T\n", next)
}
