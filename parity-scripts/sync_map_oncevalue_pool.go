package main

import (
	"bytes"
	"fmt"
	"sort"
	"sync"
)

var cfg = sync.OnceValue(func() map[string]int {
	fmt.Println("load cfg")
	return map[string]int{"port": 80}
})

func main() {
	var m sync.Map
	m.Store("a", 1)
	m.Store(2, "two")
	v, ok := m.Load("a")
	fmt.Println(v, ok)
	v, ok = m.Load("zz")
	fmt.Println(v, ok)
	act, loaded := m.LoadOrStore("a", 5)
	fmt.Println(act, loaded)
	act, loaded = m.LoadOrStore("b", 6)
	fmt.Println(act, loaded)
	fmt.Println(m.CompareAndSwap("b", 6, 7), m.CompareAndSwap("b", 6, 8))
	prev, ld := m.Swap("b", 9)
	fmt.Println(prev, ld)
	m.Delete(2)
	var keys []string
	m.Range(func(k, v any) bool {
		keys = append(keys, fmt.Sprint(k, "=", v))
		return true
	})
	sort.Strings(keys)
	fmt.Println(keys)
	vv, ld2 := m.LoadAndDelete("a")
	fmt.Println(vv, ld2, m.CompareAndDelete("b", 9))
	fmt.Println(cfg()["port"], cfg()["port"])
	f := sync.OnceFunc(func() { fmt.Println("once func") })
	f()
	f()
	vals := sync.OnceValues(func() (int, error) { fmt.Println("compute"); return 42, nil })
	fmt.Println(vals())
	fmt.Println(vals())
	pool := sync.Pool{New: func() any { fmt.Println("new buf"); return new(bytes.Buffer) }}
	b := pool.Get().(*bytes.Buffer)
	b.WriteString("hi")
	pool.Put(b)
	b2 := pool.Get().(*bytes.Buffer)
	fmt.Println(b2.String())
	var wg sync.WaitGroup
	var mu sync.Mutex
	sum := 0
	for i := 1; i <= 10; i++ {
		wg.Go(func() {
			mu.Lock()
			sum += i
			mu.Unlock()
		})
	}
	wg.Wait()
	fmt.Println(sum)
	defer func() { fmt.Println("rec:", recover()) }()
	wg.Done()
}
