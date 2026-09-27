package main

import "fmt"

// A pointer-receiver method on a defined non-struct type rebinds the value
// (`*s = append(*s, v)`), and the caller's variable, field or element sees it.
type Stack []int

func (s *Stack) Push(v int) { *s = append(*s, v) }

func (s *Stack) Pop() int {
	old := *s
	v := old[len(old)-1]
	*s = old[:len(old)-1]
	return v
}

func (s Stack) Len() int { return len(s) }

func (s *Stack) Size() int { return s.Len() }

type Counter int

func (c *Counter) Inc() { *c = *c + 1 }

func (c Counter) Double() int { return int(c) * 2 }

type Box struct{ items Stack }

func main() {
	var s Stack
	s.Push(1)
	s.Push(2)
	s.Push(3)
	fmt.Println(s, s.Len(), s.Size())
	v := s.Pop()
	fmt.Println(v, s)
	var c Counter
	c.Inc()
	c.Inc()
	fmt.Println(c, c.Double())
	b := Box{}
	b.items.Push(7)
	fmt.Println(b.items, len(b.items))
	cs := []Counter{0, 10}
	cs[1].Inc()
	fmt.Println(cs)
}
