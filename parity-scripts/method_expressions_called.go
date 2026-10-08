// A method expression called in place (`T.m(x)`, `(*T).m(p, …)`), a pointer
// method expression as a value, and a value-receiver method value taken
// through a pointer, which copies `*p` where it is written.
package main

import "fmt"

type Acc struct{ bal int }

func (a *Acc) Dep(n int) { a.bal += n }
func (a Acc) Bal() int   { return a.bal }

type Celsius float64

func (c Celsius) F() float64 { return float64(c)*9/5 + 32 }

func main() {
	a := &Acc{}
	dep := a.Dep
	dep(5)
	dep(7)
	bal := a.Bal
	a.Dep(1)
	fmt.Println(a.bal, bal())
	f := (*Acc).Dep
	f(a, 100)
	(*Acc).Dep(a, 1000)
	fmt.Println(Acc.Bal(*a), (*Acc).Bal(a))
	g := Acc.Bal
	fmt.Println(g(Acc{7}))
	fmt.Println(Celsius.F(100), Celsius(0).F())
}
