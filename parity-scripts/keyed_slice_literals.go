package main

import "fmt"

type Kind uint8

const (
	Invalid Kind = iota
	Bool
	Int
	String
)

var kindNames = []string{
	Invalid: "invalid",
	String:  "string",
	Bool:    "bool",
}

var grid = [][2]int{{1, 2}, 3: {7, 8}}

const huge = 1 << 70 >> 68

func main() {
	fmt.Printf("%d %q\n", len(kindNames), kindNames)
	fmt.Println(grid, len(grid))
	fmt.Println([]int{5: 1, 2, 0: 9})
	var a [huge]int
	fmt.Println(len(a), a)
}
