package main

// A missing key yields the map's *value type's* zero, not a universal one. The
// map carries no type at run time, so the zero has to be decided where the type
// is known — at the index site — and every value type has a different one:
// `""`, `false`, `0.0`, a nil slice, a nil map, a nil pointer, a nil interface,
// a zero struct, a zeroed array. The comma-ok form yields the same zero and
// must still report `false`.

import "fmt"

type point struct {
	X, Y int
}

type wrapper struct {
	P    point
	Tags []string
	Next *wrapper
}

type celsius float64

type ints []int

type stringer interface{ String() string }

func main() {
	// Every value type's zero, read through a key that is not there.
	fmt.Printf("int    %v\n", map[string]int{"a": 1}["zz"])
	fmt.Printf("int8   %v\n", map[string]int8{"a": 1}["zz"])
	fmt.Printf("uint64 %v\n", map[string]uint64{"a": 1}["zz"])
	fmt.Printf("float  %v\n", map[string]float64{"a": 1.5}["zz"])
	fmt.Printf("f32    %v\n", map[string]float32{"a": 1.5}["zz"])
	fmt.Printf("string %q\n", map[string]string{"a": "x"}["zz"])
	fmt.Printf("bool   %v\n", map[string]bool{"a": true}["zz"])
	fmt.Printf("rune   %v\n", map[string]rune{"a": 'x'}["zz"])

	// Reference types zero to nil, which is not the same as empty: `== nil`
	// must be true, and `%v` still prints the empty form.
	sl := map[string][]int{"a": []int{1}}
	fmt.Printf("slice  %v %v %d\n", sl["zz"], sl["zz"] == nil, len(sl["zz"]))
	mm := map[string]map[string]int{"a": map[string]int{"k": 1}}
	fmt.Printf("map    %v %v %d\n", mm["zz"], mm["zz"] == nil, len(mm["zz"]))
	pp := map[string]*point{"a": &point{1, 2}}
	fmt.Printf("ptr    %v %v\n", pp["zz"], pp["zz"] == nil)
	ii := map[string]interface{}{"a": 1}
	fmt.Printf("iface  %v %v\n", ii["zz"], ii["zz"] == nil)
	ee := map[string]error{"a": fmt.Errorf("boom")}
	fmt.Printf("error  %v %v\n", ee["zz"], ee["zz"] == nil)
	var st stringer
	ss := map[string]stringer{}
	fmt.Printf("named  %v %v\n", ss["zz"], ss["zz"] == st)

	// A struct value zeroes field by field, including its own reference fields.
	pt := map[string]point{"a": point{1, 2}}
	fmt.Printf("struct %v %+v %d\n", pt["zz"], pt["zz"], pt["zz"].X)
	wr := map[string]wrapper{"a": wrapper{point{1, 2}, []string{"t"}, nil}}
	z := wr["zz"]
	fmt.Printf("nested %v %v %v %v\n", z.P, z.Tags == nil, z.Next == nil, len(z.Tags))

	// A fixed-size array zeroes every element; a defined type zeroes as its
	// underlying type does.
	ar := map[string][2]int{"a": [2]int{1, 2}}
	fmt.Printf("array  %v %d\n", ar["zz"], len(ar["zz"]))
	ce := map[string]celsius{"a": 21.5}
	fmt.Printf("named2 %v\n", ce["zz"])
	dn := map[string]ints{"a": ints{1}}
	fmt.Printf("named3 %v %v\n", dn["zz"], dn["zz"] == nil)

	// Comma-ok reports absence and yields the same zero.
	s, ok := map[string]string{"a": "x"}["zz"]
	fmt.Printf("ok-str %q %v\n", s, ok)
	b, ok2 := map[string]bool{"a": true}["zz"]
	fmt.Printf("ok-bool %v %v\n", b, ok2)
	p, ok3 := pt["zz"]
	fmt.Printf("ok-struct %v %v\n", p, ok3)
	l, ok4 := sl["zz"]
	fmt.Printf("ok-slice %v %v %v\n", l, l == nil, ok4)

	// Comma-ok on a key that IS there still reports true and the stored value.
	s2, ok5 := map[string]string{"a": "x"}["a"]
	fmt.Printf("hit    %q %v\n", s2, ok5)
	p2, ok6 := pt["a"]
	fmt.Printf("hit2   %v %v\n", p2, ok6)

	// A nil map reads like an empty one: every key is absent, with the same
	// typed zero, and comma-ok says false.
	var nilmap map[string]string
	nv, nok := nilmap["a"]
	fmt.Printf("nilmap %q %v %d\n", nv, nok, len(nilmap))
	var nilstruct map[string]point
	fmt.Printf("nilstruct %v\n", nilstruct["a"])

	// A stored zero is indistinguishable from a miss by value alone — comma-ok
	// is the only thing that tells them apart.
	stored := map[string]string{"empty": ""}
	sv, sok := stored["empty"]
	mv, mok := stored["missing"]
	fmt.Printf("stored %q %v / %q %v\n", sv, sok, mv, mok)

	// The zero participates in arithmetic and concatenation as its own type.
	counts := map[string]int{}
	counts["a"] += 2
	counts["a"]++
	fmt.Printf("accum  %d %d\n", counts["a"], counts["b"])
	joined := map[string]string{}
	joined["k"] += "ab"
	joined["k"] += "cd"
	fmt.Printf("concat %q %q\n", joined["k"], joined["gone"])
	grouped := map[string][]int{}
	grouped["g"] = append(grouped["g"], 1, 2)
	fmt.Printf("append %v %v\n", grouped["g"], grouped["h"])

	// Reading a missing key does not create it.
	probe := map[string]int{}
	_ = probe["absent"]
	fmt.Printf("nogrow %d\n", len(probe))

	// The zero of a map read through a struct field and a nested map.
	type holder struct{ M map[string]point }
	h := holder{M: map[string]point{}}
	fmt.Printf("field  %v\n", h.M["zz"])
	deep := map[string]map[string]string{"a": map[string]string{}}
	fmt.Printf("deep   %q %q\n", deep["a"]["zz"], deep["zz"]["zz"])

	// A non-string key type reaches the same paths.
	byint := map[int][]string{1: []string{"a"}}
	fmt.Printf("intkey %v %v\n", byint[99], byint[99] == nil)
	byarr := map[[2]int]string{[2]int{1, 2}: "x"}
	fmt.Printf("arrkey %q\n", byarr[[2]int{9, 9}])
}
