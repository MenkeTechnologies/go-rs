package main

import (
	"bufio"
	"fmt"
	"io"
	"strings"
)

// `strings.NewReader` is Go's own `strings/reader.go`: an `io.Reader` that a
// `bufio.Scanner` / `bufio.Reader` reads, plus its rune, seek and `WriteTo` half.
func count(r io.Reader) int {
	sc := bufio.NewScanner(r)
	sc.Split(bufio.ScanWords)
	n := 0
	for sc.Scan() {
		n++
	}
	return n
}

func main() {
	fmt.Println(count(strings.NewReader("a b c\nd e")))
	r := strings.NewReader("héllo")
	fmt.Println(r.Len(), r.Size())
	ch, sz, err := r.ReadRune()
	fmt.Println(string(ch), sz, err)
	ch, sz, err = r.ReadRune()
	fmt.Println(string(ch), sz, err, r.Len())
	fmt.Println(r.UnreadRune(), r.UnreadRune())
	b, _ := r.ReadByte()
	fmt.Println(b)
	pos, err := r.Seek(-2, io.SeekEnd)
	fmt.Println(pos, err)
	rest, _ := io.ReadAll(r)
	fmt.Printf("%q\n", rest)
	_, err = r.Seek(0, 7)
	fmt.Println(err)
	r.Reset("xyz")
	buf := make([]byte, 2)
	n, err := r.ReadAt(buf, 2)
	fmt.Println(n, err, string(buf[:n]))
	var sb strings.Builder
	m, err := r.WriteTo(&sb)
	fmt.Println(m, err, sb.String())
	br := bufio.NewReader(strings.NewReader("line1\nline2\n"))
	for {
		line, err := br.ReadString('\n')
		if err != nil {
			break
		}
		fmt.Printf("%q\n", line)
	}
}
