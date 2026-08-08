// strconv's (value, error) results: the error text, the value returned
// alongside it, and the saturation Go reports for an out-of-range input.
package main

import (
	"fmt"
	"strconv"
)

func main() {
	for _, s := range []string{"100", "0", "-3", "+7", "xx", " 5", "5 ", "", "1.5", "999999999999999999999", "-999999999999999999999"} {
		n, err := strconv.Atoi(s)
		fmt.Printf("Atoi(%q) = %d, %v\n", s, n, err)
	}

	for _, s := range []string{"ff", "100", "7f", "zz", ""} {
		n, err := strconv.ParseInt(s, 16, 64)
		fmt.Printf("ParseInt(%q, 16) = %d, %v\n", s, n, err)
	}
	n, err := strconv.ParseInt("1010", 2, 64)
	fmt.Println(n, err)

	for _, s := range []string{"3.5", "-0.25", "1e10", "xx", "1e400", "-1e400"} {
		f, err := strconv.ParseFloat(s, 64)
		fmt.Printf("ParseFloat(%q) = %v, %v\n", s, f, err)
	}

	// The comma-ok shape callers actually write.
	if v, err := strconv.Atoi("21"); err == nil {
		fmt.Println("doubled:", v*2)
	}
	if _, err := strconv.Atoi("nope"); err != nil {
		fmt.Println("rejected:", err)
	}

	// A failed conversion's error is a real error value: non-nil, printable,
	// and distinct from every other one.
	e1, e2 := errOf("a"), errOf("a")
	fmt.Println(e1 == e2, e1 != nil, e1)

	// The single-value functions are unchanged.
	fmt.Println(strconv.Itoa(-42), strconv.FormatInt(255, 16), strconv.Quote("hi\n"))
}

func errOf(s string) error {
	_, err := strconv.Atoi(s)
	return err
}
