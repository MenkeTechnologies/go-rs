package main

import (
	"errors"
	"fmt"
	"os"
)

func a(x int) int {
	if x > 0 {
		return 1
	} else if x < 0 {
		return -1
	} else {
		return 0
	}
}
func b(x int) int {
	for {
		if x > 3 {
			return x
		}
		x++
	}
}
func c(x int) int {
	switch {
	case x > 0:
		return 1
	case x < 0:
		fallthrough
	default:
		return 2
	}
}
func d(x interface{}) string {
	switch x.(type) {
	case int:
		return "i"
	default:
		panic("no")
	}
}
func e(ch chan int) int {
	select {
	case v := <-ch:
		return v
	}
}
func f() int {
	panic("x")
}
func g(x int) (n int, err error) {
	if x == 0 {
		return
	}
	return x, errors.New("e")
}
func h() (int, string) { return i() }
func i() (int, string) { return 1, "a" }
func j(x int) int {
L:
	for {
		for {
			if x > 0 {
				goto L2
			}
			continue L
		}
	}
L2:
	return x
}
func k(x int) int {
	{
		return x
	}
}
func l(x int) int {
	goto end
end:
	return x
}
func m() int {
	for {
		select {}
	}
}
func n(x int) int {
outer:
	for {
		switch x {
		case 1:
			continue outer
		default:
			return 1
		}
	}
}
func o() {
	return
}
func p() func() int {
	return func() int {
		for {
		}
	}
}
func q(x int) int {
	switch x {
	case 1:
		return 1
	default:
		for {
			if x > 2 {
				return x
			}
		}
	}
}

func main() {
	fmt.Println(a(1), b(1), c(1), d(1), f2())
	fmt.Println(h())
	_, _ = g(0)
	_ = os.Args
	_ = j
	_ = k
	_ = l
	_ = m
	_ = n
	o()
	_ = p
	_ = q
	_ = e
}
func f2() int { return f3() }
func f3() int { return 3 }
