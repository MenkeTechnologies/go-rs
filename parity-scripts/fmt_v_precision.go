package main

// A precision under %v is the %g precision for a float (significant digits),
// the minimum digit count for an integer and a truncation for a string, and
// it applies at each leaf of a composite; %+v never prints a sign.

import "fmt"

type pair struct {
	A float64
	B string
}

func main() {
	fmt.Println(fmt.Sprintf("%10.4v|%-10v|", 3.14159265, "x"))
	fmt.Println(fmt.Sprintf("%.3v|%.0v|%.10v", 2.0/3, 1234.5, 1.0/3))
	fmt.Println(fmt.Sprintf("%.2v", []float64{3.14159, 2.71828}))
	fmt.Println(fmt.Sprintf("%8.3v|", float32(3.14159)))
	fmt.Println(fmt.Sprintf("%+.3v", 3.14159))
	fmt.Println(fmt.Sprintf("%.3v", 1e21))
	fmt.Println(fmt.Sprintf("%.3v", "abcdef"))
	fmt.Println(fmt.Sprintf("%.3v", 12345))
	fmt.Println(fmt.Sprintf("%010.3v", -3.14159))
	fmt.Println(fmt.Sprintf("%.2v", map[string]float64{"a": 1.23456}))
	fmt.Println(fmt.Sprintf("%.2v", []string{"abc", "de", "f"}))
	fmt.Println(fmt.Sprintf("%.3v", []int{5, 1234}))
	fmt.Println(fmt.Sprintf("%.1v", [][]float64{{1.25, 2.5}}))
	fmt.Println(fmt.Sprintf("%6.2v|", []float64{3.14159}))
	fmt.Println(fmt.Sprintf("%.2v", pair{1.2345, "xyz"}))
	fmt.Println(fmt.Sprintf("%+.2v", pair{1.2345, "xyz"}))
	fmt.Println(fmt.Sprintf("%.2v", []any{1.2345, "xyz", 7}))
	fmt.Println(fmt.Sprintf("%+.3v|%+5v|%+05v|%+v", 5, 7, 9, -3))
	fmt.Println(fmt.Sprintf("%+6v|", struct {
		A int
		B []int
	}{1, []int{2}}))
	fmt.Println(fmt.Sprintf("%+.1v", map[string]struct{ X float64 }{"k": {2.25}}))
	fmt.Println(fmt.Sprintf("%+v", struct{ A int }{-1}))
	fmt.Println(fmt.Sprintf("%-8v|%08v|", 3.5, -2.25))
}
