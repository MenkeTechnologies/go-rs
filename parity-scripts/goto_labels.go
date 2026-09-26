// `goto`: a backward jump that forms a loop, a forward jump that skips code,
// a jump out of nested loops, a label that ends its block, a label shared by
// a `goto` and a labeled `break`, and labels in functions, methods and
// closures — each resolved within its own function.
package main

import "fmt"

func gcd(a, b int) int {
loop:
	if b != 0 {
		a, b = b, a%b
		goto loop
	}
	return a
}

func find(grid [][]int, want int) (int, int) {
	for i, row := range grid {
		for j, v := range row {
			if v == want {
				fmt.Println("found at", i, j)
				goto done
			}
		}
	}
	fmt.Println("not found")
	return -1, -1
done:
	return 0, 0
}

type counter struct{ n int }

func (c *counter) bump(limit int) {
again:
	c.n++
	if c.n < limit {
		goto again
	}
}

func classify(n int) string {
	if n < 0 {
		goto negative
	}
	if n == 0 {
		goto zero
	}
	return "positive"
negative:
	return "negative"
zero:
	return "zero"
}

func main() {
	fmt.Println(gcd(48, 18), gcd(17, 5))

	find([][]int{{1, 2}, {3, 4}}, 3)
	find([][]int{{1, 2}}, 9)

	c := &counter{}
	c.bump(5)
	fmt.Println(c.n)

	fmt.Println(classify(-3), classify(0), classify(8))

	i := 0
top:
	for {
		i++
		switch {
		case i < 3:
			continue
		case i == 5:
			break top
		}
		if i == 4 {
			goto top
		}
	}
	fmt.Println("i =", i)

	sum := 0
	add := func(xs ...int) int {
		k := 0
	next:
		if k < len(xs) {
			sum += xs[k]
			k++
			goto next
		}
		return sum
	}
	fmt.Println(add(1, 2, 3, 4))

	n := 0
	{
		n++
		if n < 3 {
			goto skip
		}
		n = 100
	skip:
	}
	fmt.Println(n)
}
