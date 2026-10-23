// fmt's scanning family over in-program input — Sscan / Sscanln / Sscanf and
// Fscan / Fscanln / Fscanf over an io.Reader — with every operand kind the
// type switch in Go's scanOne names, the verbs and widths Sscanf takes, and
// the errors a mismatch reports.
package main

import (
	"bufio"
	"fmt"
	"io"
)

type lines struct {
	data []byte
}

func (l *lines) Read(p []byte) (int, error) {
	if len(l.data) == 0 {
		return 0, io.EOF
	}
	n := copy(p, l.data[:1]) // one byte at a time: short reads are legal
	l.data = l.data[n:]
	return n, nil
}

func main() {
	// Sscan: newlines are spaces; every basic pointer kind.
	var (
		i   int
		i8  int8
		i64 int64
		u   uint
		u16 uint16
		f   float64
		f32 float32
		s   string
		b   bool
		bs  []byte
	)
	n, err := fmt.Sscan("42 -7 9000000000\n17 65535 3.25 1.5 word true bytes", &i, &i8, &i64, &u, &u16, &f, &f32, &s, &b, &bs)
	fmt.Println(n, err, i, i8, i64, u, u16, f, f32, s, b, string(bs))

	// Base prefixes and underscores under %v.
	var x, y, z, w int
	n, err = fmt.Sscan("0x1F 0b101 0o17 1_000", &x, &y, &z, &w)
	fmt.Println(n, err, x, y, z, w)

	// Too little input, a bad token, overflow.
	n, err = fmt.Sscan("5", &x, &y)
	fmt.Println(n, err)
	n, err = fmt.Sscan("abc", &x)
	fmt.Println(n, err)
	n, err = fmt.Sscan("300", &i8)
	fmt.Println(n, err)
	n, err = fmt.Sscan("maybe", &b)
	fmt.Println(n, err)
	n, err = fmt.Sscan("1e400", &f)
	fmt.Println(n, err)

	// Sscanln stops at the newline.
	var a1, a2 int
	n, err = fmt.Sscanln("1\n2", &a1, &a2)
	fmt.Println(n, err, a1)
	n, err = fmt.Sscanln("1 2 3", &a1, &a2)
	fmt.Println(n, err, a1, a2)
	n, err = fmt.Sscanln("8 9\n", &a1, &a2)
	fmt.Println(n, err, a1, a2)

	// Sscanf: literals, verbs, widths, %c, %q, %x, %%.
	var name string
	var age int
	n, err = fmt.Sscanf("name=bob age=31", "name=%s age=%d", &name, &age)
	fmt.Println(n, err, name, age)
	var hx, oc, bn int
	n, err = fmt.Sscanf("ff 17 101", "%x %o %b", &hx, &oc, &bn)
	fmt.Println(n, err, hx, oc, bn)
	var d1, d2 int
	n, err = fmt.Sscanf("12345", "%2d%3d", &d1, &d2)
	fmt.Println(n, err, d1, d2)
	var r rune
	var c2 int
	n, err = fmt.Sscanf("Zq", "%c%c", &r, &c2)
	fmt.Println(n, err, string(r), c2)
	var q string
	n, err = fmt.Sscanf(`"hi\tthere" rest`, "%q", &q)
	fmt.Printf("%d %v %q\n", n, err, q)
	var hs string
	n, err = fmt.Sscanf("68656c6c6f", "%x", &hs)
	fmt.Println(n, err, hs)
	var pct int
	n, err = fmt.Sscanf("50%", "%d%%", &pct)
	fmt.Println(n, err, pct)
	n, err = fmt.Sscanf("7 8", "%d,%d", &d1, &d2)
	fmt.Println(n, err, d1)
	n, err = fmt.Sscanf("1", "%d %d", &d1, &d2)
	fmt.Println(n, err)
	n, err = fmt.Sscanf("1 2", "%d", &d1, &d2)
	fmt.Println(n, err)
	n, err = fmt.Sscanf("1", "%d %d", &d1)
	fmt.Println(n, err)
	var fl float64
	n, err = fmt.Sscanf("pi=3.14159", "pi=%f", &fl)
	fmt.Println(n, err, fl)
	var tf bool
	n, err = fmt.Sscanf("T", "%t", &tf)
	fmt.Println(n, err, tf)
	n, err = fmt.Sscanf("1.5", "%d", &d1)
	fmt.Println(n, err, d1)
	n, err = fmt.Sscan("12", i)
	fmt.Println(n, err)

	// Fscan over a reader that is no io.RuneScanner (wrapped in readRune) and
	// over a *bufio.Reader (one, used directly).
	var p, qq, rr int
	n, err = fmt.Fscan(&lines{[]byte("10 20\n30")}, &p, &qq, &rr)
	fmt.Println(n, err, p+qq+rr)
	br := bufio.NewReader(&lines{[]byte("1 2\n3 4\nword\n")})
	sum := 0
	for {
		var v1, v2 int
		n, err := fmt.Fscanln(br, &v1, &v2)
		if err != nil {
			fmt.Println("stop:", n, err)
			break
		}
		sum += v1 * v2
	}
	fmt.Println(sum)
	var wd string
	n, err = fmt.Fscanf(br, "%s\n", &wd)
	fmt.Println(n, err, wd)
	n, err = fmt.Fscan(br, &wd)
	fmt.Println(n, err)
}
