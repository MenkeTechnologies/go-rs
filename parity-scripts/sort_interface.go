package main

import (
	"fmt"
	"sort"
)

// sort.Sort, sort.Slice and the rest leave equal elements in the order Go's
// pattern-defeating quicksort does, which is observable: every input below has
// ties. The sizes straddle pdqsort's insertion-sort cutoff (12) and its
// ninther cutoff (50), and the descending, organ-pipe and sawtooth shapes
// steer its pivot choice.

type item struct {
	key, id int
}

type byKey []item

func (b byKey) Len() int           { return len(b) }
func (b byKey) Less(i, j int) bool { return b[i].key < b[j].key }
func (b byKey) Swap(i, j int)      { b[i], b[j] = b[j], b[i] }

func gen(n, mod, seed int) []item {
	out := make([]item, n)
	x := seed
	for i := range out {
		x = (x*1103515245 + 12345) % 2147483648
		out[i] = item{x % mod, i}
	}
	return out
}

func ids(xs []item) []int {
	r := make([]int, len(xs))
	for i, x := range xs {
		r[i] = x.id
	}
	return r
}

func main() {
	for _, n := range []int{5, 13, 49, 50, 51, 120, 300} {
		for _, mod := range []int{2, 7, 1000} {
			a := gen(n, mod, n+mod)
			sort.Sort(byKey(a))
			fmt.Println(n, mod, ids(a), sort.IsSorted(byKey(a)))
			b := gen(n, mod, n*mod)
			sort.Slice(b, func(i, j int) bool { return b[i].key < b[j].key })
			fmt.Println(ids(b))
			c := gen(n, mod, 7)
			sort.SliceStable(c, func(i, j int) bool { return c[i].key < c[j].key })
			fmt.Println(ids(c))
			d := gen(n, mod, 9)
			sort.Stable(byKey(d))
			fmt.Println(ids(d))
		}
	}
	// descending, ascending, organ-pipe and saw inputs with ties
	for _, n := range []int{20, 64, 200} {
		desc := make([]item, n)
		pipe := make([]item, n)
		saw := make([]item, n)
		for i := 0; i < n; i++ {
			desc[i] = item{(n - i) / 3, i}
			if i < n/2 {
				pipe[i] = item{i / 2, i}
			} else {
				pipe[i] = item{(n - i) / 2, i}
			}
			saw[i] = item{i % 5, i}
		}
		sort.Sort(byKey(desc))
		sort.Sort(byKey(pipe))
		sort.Sort(sort.Reverse(byKey(saw)))
		fmt.Println(ids(desc))
		fmt.Println(ids(pipe))
		fmt.Println(ids(saw))
	}
	xs := []int{5, 2, 8, 1, 9, 3}
	sort.Sort(sort.Reverse(sort.IntSlice(xs)))
	fmt.Println(xs)
	ss := sort.StringSlice{"pear", "apple", "fig"}
	ss.Sort()
	fmt.Println(ss, sort.IsSorted(ss))
	fs := []float64{2.5, -1, 3}
	sort.Sort(sort.Float64Slice(fs))
	fmt.Println(fs)
	var iface sort.Interface = sort.IntSlice([]int{3, 1, 2})
	sort.Sort(iface)
	fmt.Println(iface, iface.Len())
	fmt.Println(sort.SliceIsSorted(xs, func(i, j int) bool { return xs[i] > xs[j] }))
}
