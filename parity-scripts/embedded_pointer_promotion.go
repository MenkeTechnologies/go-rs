// Fields and methods reached through an embedded *pointer*, read and written,
// at one and two embedding depths. The embedded value is a pointer object, so a
// promoted-field lookup has to look through it.
package main

import "fmt"

type Base struct{ name string }

func (b Base) Name() string     { return b.name }
func (b *Base) Rename(n string) { b.name = n }

type Dog struct {
	Base
	age int
}

type Puppy struct {
	*Dog
	toy string
}

type Pack struct {
	*Puppy
	size int
}

func main() {
	d := Dog{Base{"rex"}, 3}
	p := Puppy{&d, "ball"}
	fmt.Println(p.age, p.name, p.toy, p.Dog.age)
	p.age = 4
	p.Rename("max")
	fmt.Println(d.age, d.name, p.Name())

	pk := Pack{&p, 2}
	pk.age += 10
	pk.toy = "rope"
	fmt.Println(pk.age, pk.name, pk.toy, d.age, p.toy, pk.size)

	lit := Puppy{Dog: &Dog{age: 7}}
	fmt.Println(lit.age, lit.name == "")
}
