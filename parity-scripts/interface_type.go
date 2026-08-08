package main

// `interface{}` used as a type rather than in a declaration: variables, slice
// and map elements, parameters and results. go-rs's parser only accepted
// `interface` in a `type X interface { … }` declaration.
import "fmt"

type stringer interface{ Describe() string }

type tag struct{ Name string }

func (t tag) Describe() string { return "tag:" + t.Name }

func echo(v interface{}) string { return fmt.Sprintf("%v/%T", v, v) }

func pick(b bool) interface{} {
	if b {
		return 1
	}
	return "one"
}

func main() {
	var i interface{} = 5
	fmt.Println(i, echo(i))

	var s interface{}
	fmt.Println(s == nil)
	s = "text"
	fmt.Println(s, echo(s))

	xs := []interface{}{1, "a", 2.5, true}
	for _, v := range xs {
		fmt.Println(echo(v))
	}

	m := map[string]interface{}{"n": 1, "s": "x"}
	fmt.Println(echo(m["n"]), echo(m["s"]))

	fmt.Println(echo(pick(true)), echo(pick(false)))

	// A type switch and a comma-ok assertion over an empty-interface value.
	for _, v := range xs {
		switch t := v.(type) {
		case int:
			fmt.Println("int", t, t/2)
		case string:
			fmt.Println("string", t)
		case float64:
			fmt.Println("float64", t, t/2)
		default:
			fmt.Println("other", t)
		}
	}
	if n, ok := xs[0].(int); ok {
		fmt.Println("asserted", n)
	}
	_, ok := xs[0].(string)
	fmt.Println(ok)

	// A named interface still dispatches its method set.
	var d stringer = tag{"x"}
	fmt.Println(d.Describe())
}
