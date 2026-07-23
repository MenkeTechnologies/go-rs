package main

import "fmt"

func generate(out chan int, n int) {
	for i := 2; i <= n; i++ {
		out <- i
	}
	close(out)
}

func main() {
	nums := make(chan int, 100)
	go generate(nums, 20)
	sum := 0
	count := 0
	for {
		v := <-nums
		if v == 0 {
			break
		}
		sum += v
		count++
		if count >= 19 {
			break
		}
	}
	fmt.Println("sum 2..20 =", sum)
}
