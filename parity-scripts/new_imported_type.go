package main

import (
	"bytes"
	"fmt"
	"strings"
	"sync"
	"time"
)

// `new(pkg.T)` of an imported package's type: a struct is a zeroed struct
// whose methods work, and a defined scalar a variable of its own.
func main() {
	b := new(bytes.Buffer)
	fmt.Println(b.Len(), b.String() == "")
	b.WriteString("x")
	fmt.Println(b.Len(), b.String())
	sb := new(strings.Builder)
	sb.WriteString("yz")
	fmt.Println(sb.String(), sb.Len())
	wg := new(sync.WaitGroup)
	mu := new(sync.Mutex)
	n := 0
	for i := 0; i < 3; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			mu.Lock()
			n++
			mu.Unlock()
		}()
	}
	wg.Wait()
	fmt.Println(n)
	d := new(time.Duration)
	*d += 1500 * time.Millisecond
	fmt.Println(*d)
	fmt.Printf("%T %T %T\n", b, sb, d)
}
