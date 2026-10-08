// `fmt` calls String()/Error() on a struct's *exported* fields as it prints
// them — an enum field, an error held in an interface field, a slice of
// enums, a struct nested in a struct — under the verbs that print text, and
// leaves an unexported field as its plain value. A field named like its type
// (`Kind Kind`) is a named field, not an embedded one, so it promotes nothing.
package main

import (
	"errors"
	"fmt"
)

type Kind int

const (
	Num Kind = iota
	Op
)

func (k Kind) String() string { return [...]string{"NUM", "OP"}[k] }

type Token struct {
	Kind Kind
	Text string
}

type hidden struct {
	kind Kind
	Text string
}

type Result struct {
	Err   error
	Kinds []Kind
	First Token
	Any   any
	n     int
}

type Wrapper struct {
	Inner Token
	Toks  []Token
}

func main() {
	t := Token{Op, "+"}
	fmt.Println(t, &t, []Token{{Num, "1"}, t})
	fmt.Printf("%v|%+v|%s|%d|%q\n", t, t, t, t, t)
	fmt.Println(hidden{Op, "x"})
	r := Result{errors.New("bad"), []Kind{Op, Num}, Token{Num, "7"}, Kind(1), 3}
	fmt.Println(r)
	fmt.Printf("%+v\n", r)
	fmt.Println(Result{}, Wrapper{Token{Op, "*"}, []Token{{Num, "2"}}})
	fmt.Println(fmt.Sprint(t), fmt.Sprintf("%v %d", []Token{t}, []Token{t}))

	var x any = Token{}
	_, ok := x.(fmt.Stringer)
	fmt.Println("Token is a Stringer:", ok)
	var a any = &Token{Op, "-"}
	switch v := a.(type) {
	case *Token:
		fmt.Println(v.Kind, v)
	}
}
