// Map key equality and the order a map keeps through inserts and deletes — the
// two things a hash index beside the ordered pairs has to leave untouched.
package main

import "fmt"

type mkKey struct {
	A int
	B string
}

type mkWd int
type mkStr string

func main() {
	m := map[string]int{"b": 3, "a": 2}
	m["c"] = 4
	delete(m, "a")
	m["a"] = 5
	m["d"] = 6
	delete(m, "d")
	fmt.Println(m, len(m))
	v, ok := m["a"]
	fmt.Println(v, ok)
	v2, ok2 := m["zz"]
	fmt.Println(v2, ok2)

	// A struct key compares field by field, so a differently-allocated one with
	// the same fields finds the entry. It also has no hash projection, which is
	// what makes this the scan path.
	sk := map[mkKey]int{{1, "x"}: 10}
	sk[mkKey{1, "x"}] = 11
	sk[mkKey{2, "y"}] = 20
	fmt.Println(len(sk), sk[mkKey{1, "x"}], sk[mkKey{2, "y"}], sk[mkKey{9, "z"}])
	delete(sk, mkKey{1, "x"})
	fmt.Println(len(sk), sk[mkKey{2, "y"}])

	ak := map[[2]int]string{{1, 2}: "a"}
	ak[[2]int{1, 2}] = "b"
	ak[[2]int{3, 4}] = "c"
	fmt.Println(len(ak), ak[[2]int{1, 2}], ak[[2]int{3, 4}])

	// A string, a bool and a nil are three distinct keys of a map[any]int.
	am := map[any]int{}
	am["1"] = 2
	am[true] = 3
	am[nil] = 4
	fmt.Println(len(am), am["1"], am[true], am[nil])

	bm := map[bool]string{true: "t", false: "f"}
	fmt.Println(bm[true], bm[false], len(bm))

	// -0.0 and 0.0 are the same key.
	fm := map[float64]int{1.5: 1, -0.0: 2}
	fm[0.0] = 3
	fmt.Println(len(fm), fm[1.5], fm[0.0], fm[-0.0])

	// An integer literal is a float64 key in a float64-keyed map.
	fl := map[float64]int{1: 5, 2.0: 6}
	fmt.Println(fl[1], fl[1.0], fl[2], len(fl))

	um := map[uint64]string{}
	um[18446744073709551615] = "max"
	um[1] = "one"
	fmt.Println(len(um), um[18446744073709551615], um[1])

	wm := map[mkWd]int{}
	wm[mkWd(3)] = 30
	wm[mkWd(4)] = 40
	fmt.Println(len(wm), wm[mkWd(3)], wm[mkWd(4)], wm[mkWd(9)])

	sm := map[mkStr]int{}
	sm[mkStr("a")] = 1
	fmt.Println(len(sm), sm[mkStr("a")])

	rm := map[rune]int{'a': 1, 'b': 2}
	fmt.Println(len(rm), rm['a'], rm['b'])

	var nm map[string]int
	fmt.Println(nm["x"], len(nm), nm == nil)

	// Past the point where the index is built, and then back through deletes:
	// the surviving pairs keep their positions.
	om := map[int]int{}
	for i := 0; i < 40; i++ {
		om[i] = i * 3
	}
	for i := 0; i < 40; i += 2 {
		delete(om, i)
	}
	tot := 0
	for k, v := range om {
		tot += k*100 + v
	}
	fmt.Println(len(om), tot, om[1], om[39], om[2])

	nested := map[string]map[string]int{"o": {"i": 1}}
	nested["o"]["j"] = 2
	fmt.Println(nested, len(nested["o"]))
}
