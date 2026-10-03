package main

import (
	"fmt"
	"math"
)

// FNV-1a: an offset basis above i64, multiplied in uint64.
const offset64 = 14695981039346656037
const prime64 = 1099511628211

func fnv(s string) uint64 {
	var h uint64 = offset64
	for i := 0; i < len(s); i++ {
		h ^= uint64(s[i])
		h *= prime64
	}
	return h
}

// Constants are exact: `1<<64` does not fit any integer type, but the
// declarations built from it do.
const big = 1<<64 - 1
const half = big >> 1
const m0 = 0x5555555555555555

type Mask uint64

const all Mask = 1<<64 - 1

func popcount(x uint64) int {
	const m = 1<<64 - 1
	x = x>>1&(m0&m) + x&(m0&m)
	x = x>>2&(0x3333333333333333&m) + x&(0x3333333333333333&m)
	x = (x>>4 + x) & (0x0f0f0f0f0f0f0f0f & m)
	x += x >> 8
	x += x >> 16
	x += x >> 32
	return int(x) & (1<<7 - 1)
}

func split(v uint64) (uint64, uint64) { return v >> 32, v & (1<<32 - 1) }

func main() {
	fmt.Println(fnv("hello"), fnv(""))
	var u uint64 = math.MaxUint64
	fmt.Println(u, uint64(math.MaxUint64), uint(math.MaxUint), ^uint64(0))
	fmt.Println(uint64(big), uint64(half), half, big&0xff, all)
	var w uint64 = 1<<64 - 1
	fmt.Println(w, w>>60, uint32(1<<32-1))
	const k = 1 << 70 >> 68
	fmt.Println(k, uint64(1<<62*2-1+(1<<62)*2))
	fmt.Println(popcount(255), popcount(1<<63|1), popcount(u))

	// A uint64 computed by `:=` from uint64 operands stays unsigned through
	// division and ordered comparison.
	hi, lo := split(u)
	sum := hi<<32 | lo
	q := sum / 3
	fmt.Println(hi, lo, sum, q, sum > q, sum%1000)
}
