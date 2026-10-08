// `fmt` uses a String/Error method promoted from an embedded field, under
// Go's method-set rule: an embedded `*T` gives the value T's pointer methods,
// an embedded `T` gives the value only T's value methods (the pointer gets
// them all), and the promotion reaches through several levels.
package main

import "fmt"

type S struct{ A int }

func (s *S) String() string { return fmt.Sprint("S!", s.A) }

type V struct{ B int }

func (v V) String() string { return fmt.Sprint("V!", v.B) }

type E struct{ code int }

func (e *E) Error() string { return fmt.Sprint("E!", e.code) }

type EmbedPtr struct{ *S }

type EmbedVal struct{ S }

type EmbedValueMethod struct {
	V
	N int
}

type Deep struct{ EmbedValueMethod }

type WithErr struct {
	*E
	V
}

type Own struct{ V }

func (o Own) String() string { return "own" }

func main() {
	fmt.Println(EmbedPtr{&S{1}}, EmbedVal{S{2}}, &EmbedVal{S{3}})
	fmt.Println(EmbedValueMethod{V{4}, 5}, &EmbedValueMethod{V{6}, 7}, Deep{EmbedValueMethod{V{8}, 9}})
	fmt.Println(WithErr{&E{10}, V{11}}, Own{V{12}})
	fmt.Printf("%v|%s|%d|%+v\n", EmbedPtr{&S{13}}, Deep{}, EmbedVal{S{14}}, EmbedVal{S{15}})
	fmt.Println([]EmbedValueMethod{{V{16}, 0}}, []*EmbedVal{{S{17}}})
	fmt.Println(fmt.Sprint(Deep{}), fmt.Sprintf("%v", &Deep{}))
}
