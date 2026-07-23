package main

import (
	"fmt"
	"strconv"
	"strings"
)

func main() {
	s := "Hello, World"
	fmt.Println(strings.ToUpper(s), strings.ToLower(s))
	fmt.Println(strings.Contains(s, "World"), strings.HasPrefix(s, "Hell"), strings.HasSuffix(s, "ld"))
	fmt.Println(strings.Replace(s, "l", "L", -1), strings.Count(s, "l"))
	parts := strings.Split("a,b,c,d", ",")
	fmt.Println(parts, strings.Join(parts, "-"), len(parts))
	fmt.Println(strings.TrimSpace("  hi  "), strings.Repeat("ab", 3))
	fmt.Println(strconv.Itoa(255), strconv.FormatInt(255, 16))
	n, _ := strconv.Atoi("42")
	fmt.Println(n + 8)
}
