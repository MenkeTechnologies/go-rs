package main

// `range` over a slice, a fixed-size array and an integer walks the index
// sequence 0 … n-1, so the loop binds `$i` directly instead of reading it back
// out of a materialized key list. A map and a string do not: a map's keys are
// its own values, and a string's are the byte offsets each rune *starts* at.
// This pins the boundary between the two lowerings, and the cases where the
// difference between "the index" and "the key list" is observable — a body that
// grows or shrinks the container it is walking, and an array's copy semantics.

import "fmt"

type point struct {
	X, Y int
}

func sum(xs ...int) int {
	t := 0
	for i, x := range xs {
		t += (i + 1) * x
	}
	return t
}

func main() {
	// Index-only over a slice, and both variables.
	s := []int{10, 20, 30}
	idx, tot := 0, 0
	for i := range s {
		idx += i
	}
	for i, v := range s {
		tot += i * v
	}
	fmt.Println("slice", idx, tot)

	// The iteration count is fixed when the loop starts: an append inside the
	// body does not lengthen the walk, and the appended elements are not seen.
	grow := []int{1, 2}
	seen := 0
	for range grow {
		grow = append(grow, 99)
		seen++
	}
	fmt.Println("grow", seen, len(grow))

	// The *element* read is not fixed: `s[i]` sees a write the body made to a
	// later index, because the slice is a shared handle.
	mut := []int{1, 2, 3}
	got := ""
	for i, v := range mut {
		if i == 0 {
			mut[2] = 42
		}
		got += fmt.Sprintf("%d:%d ", i, v)
	}
	fmt.Println("mutate", got)

	// An array is copied by the range expression, so the same write is *not*
	// seen — the one place where `[3]int` and `[]int` diverge here.
	arr := [3]int{1, 2, 3}
	agot := ""
	for i, v := range arr {
		if i == 0 {
			arr[2] = 42
		}
		agot += fmt.Sprintf("%d:%d ", i, v)
	}
	fmt.Println("array", agot, arr[2])

	// Range over an integer (Go 1.22), including the non-positive counts that
	// must run zero times rather than error.
	n := 0
	for i := range 5 {
		n += i
	}
	neg := 0
	for range -3 {
		neg++
	}
	zero := 0
	for range 0 {
		zero++
	}
	fmt.Println("int", n, neg, zero)

	// A sized integer type is still an integer range.
	var b byte = 4
	bs := 0
	for i := range b {
		bs += int(i)
	}
	var r rune = 3
	rs := 0
	for i := range r {
		rs += int(i)
	}
	fmt.Println("sized", bs, rs)

	// A nil slice ranges zero times; so does an empty map and an empty string.
	var nilslice []int
	nils := 0
	for range nilslice {
		nils++
	}
	fmt.Println("nil", nils, len(nilslice))

	// A string's keys are rune-start byte offsets, not 0 … len-1.
	offs := ""
	for i, r := range "héllo" {
		offs += fmt.Sprintf("%d/%c ", i, r)
	}
	fmt.Println("string", offs, len("héllo"))

	// A byte slice of the same text *is* indexed 0 … len-1, and yields bytes.
	bytes := ""
	for i, c := range []byte("héllo") {
		bytes += fmt.Sprintf("%d/%d ", i, c)
	}
	fmt.Println("bytes", bytes)

	// A rune slice is indexed by rune position.
	runes := ""
	for i, c := range []rune("héllo") {
		runes += fmt.Sprintf("%d/%c ", i, c)
	}
	fmt.Println("runes", runes)

	// A map's keys are its own; deleting during the walk does not change the
	// number of iterations go-rs's key snapshot produces.
	m := map[string]int{"a": 1, "b": 2, "c": 3}
	keys := 0
	for k := range m {
		delete(m, k)
		keys++
	}
	fmt.Println("map", keys, len(m))

	// The range variable is a copy, so writing through it leaves the slice
	// alone — the read-only-walk assumption Go programs are built on.
	pts := []point{{1, 2}, {3, 4}}
	for _, p := range pts {
		p.X = 99
	}
	fmt.Println("copy", pts[0].X, pts[1].X)

	// A variadic parameter is a slice inside the body, whatever its element
	// type is written as at the call site.
	fmt.Println("variadic", sum(1, 2, 3), sum(), sum([]int{4, 5}...))

	// `break` and `continue`, labeled and not, still land in the right place.
	c := 0
	for i := range 10 {
		if i%2 == 0 {
			continue
		}
		if i > 6 {
			break
		}
		c += i
	}
	fmt.Println("jumps", c)

	lab := 0
outer:
	for i := range 4 {
		for j := range 4 {
			if j > i {
				continue outer
			}
			if i*j >= 4 {
				break outer
			}
			lab++
		}
	}
	fmt.Println("labeled", lab)

	// A slice of slices: the outer index walk and the inner one nest without
	// sharing the loop counter.
	grid := [][]int{{1, 2}, {3}, {}}
	flat := 0
	for i := range grid {
		for j := range grid[i] {
			flat += grid[i][j] * (j + 1)
		}
	}
	fmt.Println("nested", flat)
}
