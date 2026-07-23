package main

import "fmt"

func worker(jobs chan int, results chan int) {
	for {
		j := <-jobs
		results <- j * j
	}
}

func main() {
	jobs := make(chan int, 20)
	results := make(chan int, 20)
	for w := 0; w < 4; w++ {
		go worker(jobs, results)
	}
	for j := 1; j <= 12; j++ {
		jobs <- j
	}
	sum := 0
	for i := 0; i < 12; i++ {
		sum += <-results
	}
	fmt.Println("sum of squares 1..12 =", sum)
}
