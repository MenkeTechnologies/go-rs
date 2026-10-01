package main

import (
	"errors"
	"fmt"
	"strconv"
)

// A declaration in an inner block that reuses an outer variable's name is a
// new variable scoped to that block; the outer one is untouched by it.

var count = 100

func describe(v any) string {
	switch v := v.(type) {
	case int:
		return "int " + strconv.Itoa(v*2)
	case string:
		return "string " + v + v
	default:
		_ = v
		return "other"
	}
}

func parse(xs []string) (total int, err error) {
	for _, s := range xs {
		n, err := strconv.Atoi(s)
		if err != nil {
			continue
		}
		total += n
	}
	if total > 5 {
		err := errors.New("inner")
		_ = err
	}
	return total, err
}

func counters() []func() int {
	var fs []func() int
	x := 0
	for i := 0; i < 3; i++ {
		x := x + i*10
		fs = append(fs, func() int { x++; return x })
	}
	return fs
}

func main() {
	// Nested loops sharing a loop variable's name.
	for j := 0; j < 3; j++ {
		for j := 10; j < 12; j++ {
			fmt.Print(j, " ")
		}
		fmt.Println("outer", j)
	}
	sum := 0
	for i := 0; i < 3; i++ {
		for i := 0; i < 2; i++ {
			sum += i
		}
		sum += i * 100
	}
	fmt.Println(sum)
	for i := 0; i < 2; i++ {
		i := i * 10
		fmt.Println(i)
	}

	// A bare block, and one shadowing a package-level variable.
	x := 1
	{
		x := 2
		x++
		_ = x
	}
	fmt.Println(x)
	{
		count := 5
		count++
		fmt.Println("inner count", count)
	}
	fmt.Println("global count", count)

	// if / else-if init statements, and a further shadow in the body.
	if x := x + 1; x > 1 {
		x := x * 10
		fmt.Println("then", x)
	} else if y := x; y > 0 {
		fmt.Println("else", x, y)
	}
	fmt.Println(x)
	err := errors.New("outer")
	if _, err := strconv.Atoi("q"); err != nil {
		fmt.Println("inner err:", err)
	}
	fmt.Println(err)

	// Type switch, named results, closures and their parameters.
	fmt.Println(describe(21), describe("ab"), describe(1.5))
	fmt.Println(parse([]string{"1", "x", "7"}))
	for _, f := range counters() {
		fmt.Println(f(), f())
	}
	f := func(x int) int {
		x *= 3
		return x
	}
	fmt.Println(f(4), x)

	// select receive binding, goroutine body, range variables.
	ch := make(chan int, 1)
	v := -1
	ch <- 7
	select {
	case v := <-ch:
		fmt.Println("got", v)
	}
	fmt.Println("v", v)
	done := make(chan bool)
	n := 0
	go func() {
		n := 50
		n++
		_ = n
		done <- true
	}()
	<-done
	fmt.Println("n", n)
	s := "abc"
	for i, s := range s {
		fmt.Print(i, string(s), " ")
	}
	fmt.Println(s)
}
