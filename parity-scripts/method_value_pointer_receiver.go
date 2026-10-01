// A method value binds its receiver when it is evaluated: a pointer-receiver
// method on an addressable struct binds &c, so calls through it mutate c,
// while a value-receiver method value keeps the copy taken at that moment.
package main

import "fmt"

type Counter struct{ n int }

func (c *Counter) Inc()      { c.n++ }
func (c Counter) Get() int   { return c.n }
func (c *Counter) Add(k int) { c.n += k }

func apply(f func(int), k int) { f(k) }

func main() {
	c := Counter{}
	inc := c.Inc
	get := c.Get
	inc()
	inc()
	fmt.Println(c.n, get(), c.Get())
	apply(c.Add, 10)
	fmt.Println(c.n)
	p := &Counter{n: 5}
	pi := p.Inc
	pi()
	fmt.Println(p.n)
	cs := []Counter{{1}, {2}}
	f := cs[1].Inc
	f()
	fmt.Println(cs)
}
