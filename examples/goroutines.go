package main

import "fmt"

// A worker doubles each job it receives and sends the result back.
func worker(jobs chan int, results chan int) {
	for {
		j := <-jobs
		results <- j * 2
	}
}

func main() {
	jobs := make(chan int, 5)
	results := make(chan int, 5)

	go worker(jobs, results)

	for i := 1; i <= 5; i++ {
		jobs <- i
	}

	sum := 0
	for i := 0; i < 5; i++ {
		sum += <-results
	}
	fmt.Println("sum of doubled jobs:", sum)

	// Unbuffered handshake between two goroutines.
	done := make(chan int)
	go announce(done)
	fmt.Println("worker said:", <-done)
}

func announce(done chan int) {
	done <- 99
}
