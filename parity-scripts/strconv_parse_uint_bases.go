package main

import (
	"errors"
	"fmt"
	"strconv"
)

func show(v any, err error) {
	var ne *strconv.NumError
	if errors.As(err, &ne) {
		fmt.Println(v, "|", ne.Func, ne.Num, ne.Err, errors.Is(err, strconv.ErrRange), errors.Is(err, strconv.ErrSyntax))
		return
	}
	fmt.Println(v, err)
}

func main() {
	for _, s := range []string{"255", "256", "18446744073709551615", "18446744073709551616", "-1", "0x_ff", "1__0", ""} {
		u, err := strconv.ParseUint(s, 0, 64)
		fmt.Println(u, err)
		u8, err := strconv.ParseUint(s, 10, 8)
		show(u8, err)
	}
	for _, s := range []string{"-0x1f", "0b101", "0o17", "017", "1_000", "_10", "128", "-129", "-128", "+42", "9223372036854775808", "-9223372036854775809", "z"} {
		n, err := strconv.ParseInt(s, 0, 8)
		show(n, err)
		m, err := strconv.ParseInt(s, 0, 64)
		show(m, err)
	}
	_, err := strconv.ParseInt("1", 1, 64)
	fmt.Println(err)
	_, err = strconv.ParseInt("1", 10, 65)
	fmt.Println(err)
	_, err = strconv.ParseUint("7", 37, 0)
	fmt.Println(err)
	for _, s := range []string{"12a", "-99999999999999999999", " 1", "+5", "0x10", "1_0"} {
		n, err := strconv.Atoi(s)
		show(n, err)
	}
	fmt.Println(strconv.FormatUint(18446744073709551615, 2), strconv.FormatUint(255, 16), strconv.FormatUint(0, 36), strconv.IntSize)
	var big uint64 = 1<<63 + 5
	fmt.Println(strconv.FormatUint(big, 10), strconv.FormatInt(-255, 16))
}
