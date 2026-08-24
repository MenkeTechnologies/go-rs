// Every `strconv.FormatFloat` combination that matters: 31 values across the
// float range x the five decimal verbs x seven precisions x both bit sizes.
//
// `bitSize` 32 rounds the value BEFORE formatting, which is visible in an
// explicit precision and in an overflow to +Inf — honouring it only on the
// shortest-representation path leaves the other 5/7 of the table wrong.
package main

import (
	"fmt"
	"strconv"
)

func main() {
	vals := []float64{0, 1, -1, 0.5, -0.5, 1e-300, 1e300, 1.0 / 3.0, 2.0 / 3.0, 1e-7, 1e7, 1e21, 1e-21,
		123456789.123456789, 0.000123456, 99999999999999999999.0, 3.0, 1024.0, 1e15, 1e16, 1e17,
		2.5, 3.5, 0.1, 0.2, 0.3, 1e-4, 1e-5, 1234.5, 65536.0, 1.7976931348623157e308, 5e-324}
	verbs := []byte{'f', 'e', 'E', 'g', 'G'}
	precs := []int{-1, 0, 1, 2, 5, 10, 17}
	bits := []int{32, 64}
	for _, v := range vals {
		for _, b := range verbs {
			for _, p := range precs {
				for _, bs := range bits {
					fmt.Printf("%s|", strconv.FormatFloat(v, b, p, bs))
				}
			}
		}
		fmt.Println()
	}
}
