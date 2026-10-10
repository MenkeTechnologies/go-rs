// Identifiers are Unicode letters and digits.
package main

import "fmt"

type Größe struct{ 幅, 高さ int }

func (g Größe) 面積() int { return g.幅 * g.高さ }

func main() {
	π := 3.14
	héllo := "wörld"
	g := Größe{3, 4}
	fmt.Println(π, héllo, len(héllo), g.面積(), g)
	for ñ := range 2 {
		fmt.Print(ñ, " ")
	}
	fmt.Println()
}
