// Faults on a nil value are recoverable `runtime.Error` panics with Go's text:
// a nil pointer's field, method or `*p`, a nil func call, a nil interface's
// method, a nil map write, and `panic(nil)`.
package main

import "fmt"

type T struct {
	N int
	S string
}

func (t *T) Get() int { return t.N }
func (t T) Val() int  { return t.N }

func try(name string, f func()) {
	defer func() { fmt.Println(name, recover()) }()
	f()
}

func main() {
	var p *T
	try("field", func() { fmt.Println(p.N) })
	try("assign", func() { p.N = 1 })
	try("method", func() { fmt.Println(p.Get()) })
	try("valmethod", func() { fmt.Println(p.Val()) })
	try("deref", func() { q := *p; fmt.Println(q) })
	var ip *int
	try("intderef", func() { fmt.Println(*ip) })
	try("intassign", func() { *ip = 3 })
	var m map[string]int
	try("nilmap", func() { m["a"] = 1 })
	var s []int
	try("idx", func() { fmt.Println(s[0]) })
	var f func()
	try("nilfunc", func() { f() })
	var e error
	try("nilerr", func() { fmt.Println(e.Error()) })
	var i interface{}
	try("assert", func() { _ = i.(int) })
	k := 5
	try("slice", func() { _ = s[1:k] })
	try("div", func() { fmt.Println(k / (k - 5)) })
	try("custom", func() { panic(fmt.Sprintf("x%d", k)) })
	try("nilpanic", func() { panic(nil) })
}
