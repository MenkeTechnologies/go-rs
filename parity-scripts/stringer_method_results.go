package main

import "fmt"

// A value of a defined type reached through a method result or an index into a
// defined slice type still prints through its `String()` method.
type Day int

type Days []Day

func (d Day) String() string { return [...]string{"Sun", "Mon", "Tue", "Wed", "Thu"}[d] }

func (d Day) Next() Day { return d + 1 }

func (ds Days) Last() Day { return ds[len(ds)-1] }

type W struct{ d Day }

func (w W) Get() Day { return w.d }

func main() {
	d := Day(1)
	fmt.Println(d.Next(), d.Next().Next())
	fmt.Printf("%T %v %d\n", d.Next(), d.Next(), d.Next())
	ds := Days{1, 3}
	fmt.Println(ds.Last(), ds[0], ds, len(ds))
	fmt.Println(W{2}.Get())
	s := fmt.Sprint(ds.Last())
	fmt.Println(s + "!")
}
