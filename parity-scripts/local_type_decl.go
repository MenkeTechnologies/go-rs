// A `type` declaration inside a function body, and the parenthesized group form
// at both levels. Go scopes a local type to its block; go-rs hoists it, so what
// this pins is that the name, its fields and its `%T` spelling survive.
package main

import (
	"fmt"
	"sort"
)

type (
	ltA int
	ltB struct{ V string }
	ltC interface{ M() int }
)

type ltShadow int

func ltInner() {
	type local struct{ V int }
	l := local{7}
	fmt.Printf("%v %+v %T\n", l, l, l)
	type myInt int
	var x myInt = 5
	fmt.Printf("%v %T\n", x+1, x)
}

func main() {
	var a ltA = 3
	b := ltB{"x"}
	var c ltC
	fmt.Printf("%v %T %v %T %v\n", a, a, b, b, c == nil)

	type Cel float64
	type key struct{ A, B int }
	type Named interface{ Name() string }

	var cel Cel = 36.6
	fmt.Printf("%v %T\n", cel, cel)

	m := map[key]int{{1, 2}: 3}
	m[key{4, 5}] = 6
	fmt.Println(len(m), m[key{1, 2}], m[key{4, 5}], m[key{9, 9}])
	fmt.Printf("%v %+v %T\n", key{1, 2}, key{1, 2}, key{1, 2})

	var n Named
	fmt.Println(n == nil)

	ks := []string{}
	for k := range m {
		ks = append(ks, fmt.Sprintf("%v", k))
	}
	sort.Strings(ks)
	fmt.Println(ks)

	type (
		gA int
		gB struct{ V string }
	)
	var ga gA = 3
	gb := gB{"y"}
	fmt.Printf("%v %T %v %T\n", ga, ga, gb, gb)

	{
		type inner struct{ N int }
		fmt.Println(inner{1})
	}
	for i := 0; i < 2; i++ {
		type loopT struct{ I int }
		fmt.Println(loopT{i})
	}

	ltInner()
}
