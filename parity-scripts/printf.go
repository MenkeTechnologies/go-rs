package main

import "fmt"

func main() {
	fmt.Printf("%.4f|%.2f|%8.2f|%-8.2f|%05.1f\n", 3.14159, 2.5, 1.5, 1.5, 3.2)
	fmt.Printf("%5d|%-5d|%05d|%+d|%x|%X|%o|%b\n", 42, 42, 42, 7, 255, 255, 8, 5)
	fmt.Printf("%10s|%-10s|%.3s|%q|%t\n", "hi", "hi", "hello", "x", true)
	fmt.Printf("%v %v %v\n", []int{1, 2, 3}, 3.0, "s")
}
