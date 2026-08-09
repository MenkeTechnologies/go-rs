// A failed `strconv` conversion returns a `*strconv.NumError` wrapping the
// `ErrSyntax` / `ErrRange` sentinel — so `errors.Is`, `errors.As`, `Unwrap` and
// the `Func`/`Num`/`Err` fields all work, not just the message text.
package main

import (
	"errors"
	"fmt"
	"strconv"
)

func main() {
	_, err := strconv.Atoi("xx")
	fmt.Println(err)
	fmt.Println(errors.Is(err, strconv.ErrSyntax), errors.Is(err, strconv.ErrRange))
	var ne *strconv.NumError
	if errors.As(err, &ne) {
		fmt.Println(ne.Func, ne.Num, ne.Err)
	}
	fmt.Println(errors.Unwrap(err) == strconv.ErrSyntax)

	_, err2 := strconv.ParseInt("99999999999999999999", 10, 64)
	fmt.Println(err2)
	fmt.Println(errors.Is(err2, strconv.ErrRange), errors.Is(err2, strconv.ErrSyntax))
	var ne2 *strconv.NumError
	fmt.Println(errors.As(err2, &ne2), ne2.Func, ne2.Num)

	_, err3 := strconv.ParseFloat("q", 64)
	fmt.Println(err3, errors.Is(err3, strconv.ErrSyntax))

	// The sentinels are values, and every mention is the same one.
	fmt.Println(strconv.ErrSyntax, strconv.ErrRange)
	fmt.Println(strconv.ErrSyntax == strconv.ErrSyntax, strconv.ErrSyntax == strconv.ErrRange)

	// A `%w` wrap keeps both the chain and the concrete type reachable.
	wrapped := fmt.Errorf("parse config: %w", err)
	fmt.Println(wrapped)
	fmt.Println(errors.Is(wrapped, strconv.ErrSyntax))
	var ne3 *strconv.NumError
	fmt.Println(errors.As(wrapped, &ne3), ne3.Num)

	// A success still returns a nil error.
	n, ok := strconv.Atoi("42")
	fmt.Println(n, ok == nil)
}
