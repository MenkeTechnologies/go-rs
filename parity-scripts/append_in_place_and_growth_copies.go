package main

// `append(dst, src...)` writes into `dst`'s backing array when it has the
// capacity — `append(s[:1], s[2:]...)` deletes in place and the original sees
// it — and, when the append outgrows the array, the new one holds *copies* of
// any struct or array elements, so writing through the grown slice leaves the
// old one alone.

import "fmt"

type P struct{ N int }

func main() {
	orig := []int{1, 2, 3, 4, 5}
	s := append(orig[:1], orig[2:]...)
	fmt.Println(s, len(s), cap(s), orig)
	t := orig[:2]
	fmt.Println(len(t), cap(t))
	t = append(t, 9)
	fmt.Println(t, cap(t), orig)
	u := orig[1:3]
	fmt.Println(cap(u))
	u = append(u, 7, 8)
	fmt.Println(u, cap(u), orig)
	v := append(orig[:0], 100)
	fmt.Println(v, cap(v), orig)
	w := orig[4:]
	w = append(w, 1)
	fmt.Println(w, cap(w), orig)
	ps := []P{{1}, {2}}
	ys := append(ps, P{3})
	ys[0].N = 100
	fmt.Println(ps, ys)
	zs := append(ps, []P{{4}}...)
	zs[1].N = 200
	fmt.Println(ps, zs)
	arr := [][2]int{{1, 2}}
	brr := append(arr, [2]int{3, 4})
	brr[0][0] = 9
	fmt.Println(arr, brr)
	crr := append(arr, arr...)
	crr[0][1] = 7
	crr[1][1] = 8
	fmt.Println(arr, crr)
}
