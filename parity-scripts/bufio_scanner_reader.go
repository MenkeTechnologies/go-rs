// bufio's read half — Scanner (every stock split function, a custom one, the
// buffer cap) and Reader (string, line, rune, byte, peek, discard) — driven by
// an in-program io.Reader so the corpus needs no stdin.
package main

import (
	"bufio"
	"fmt"
	"io"
	"strings"
)

type chunked struct {
	b    []byte
	i    int
	step int // bytes per Read, to exercise refills mid-token and mid-rune
}

func newChunked(s string, step int) *chunked { return &chunked{b: []byte(s), step: step} }

func (r *chunked) Read(p []byte) (int, error) {
	if r.i >= len(r.b) {
		return 0, io.EOF
	}
	end := r.i + r.step
	if end > len(r.b) {
		end = len(r.b)
	}
	n := copy(p, r.b[r.i:end])
	r.i += n
	return n, nil
}

func lines(s string, step int) {
	sc := bufio.NewScanner(newChunked(s, step))
	for sc.Scan() {
		fmt.Printf("%q %d\n", sc.Text(), len(sc.Bytes()))
	}
	fmt.Println("err:", sc.Err())
}

func main() {
	lines("héllo wörld\nsecond line\r\nthird", 5)
	lines("", 3)
	lines("\n\n", 3)
	lines("trailing newline\n", 1000)

	ws := bufio.NewScanner(newChunked(" the\tquick  brown\nfox ", 4))
	ws.Split(bufio.ScanWords)
	for ws.Scan() {
		fmt.Printf("[%s]", ws.Text())
	}
	fmt.Println(ws.Err())

	rs := bufio.NewScanner(newChunked("aé€", 2))
	rs.Split(bufio.ScanRunes)
	for rs.Scan() {
		fmt.Printf("%q ", rs.Text())
	}
	fmt.Println(rs.Err())

	bs := bufio.NewScanner(newChunked("ab", 1))
	bs.Split(bufio.ScanBytes)
	for bs.Scan() {
		fmt.Print(bs.Bytes(), " ")
	}
	fmt.Println(bs.Err())

	// A custom split: comma-separated fields, the last one ending at EOF.
	cs := bufio.NewScanner(newChunked("a,bb,,ccc", 3))
	cs.Split(func(data []byte, atEOF bool) (int, []byte, error) {
		for i, b := range data {
			if b == ',' {
				return i + 1, data[:i], nil
			}
		}
		if atEOF && len(data) > 0 {
			return len(data), data, nil
		}
		return 0, nil, nil
	})
	for cs.Scan() {
		fmt.Printf("%q ", cs.Text())
	}
	fmt.Println(cs.Err())

	// A token past MaxScanTokenSize stops the scan with ErrTooLong; a Buffer
	// call raises the cap.
	long := strings.Repeat("x", bufio.MaxScanTokenSize+1)
	ts := bufio.NewScanner(newChunked(long, 4096))
	for ts.Scan() {
		fmt.Println("tok", len(ts.Text()))
	}
	fmt.Println(ts.Err(), ts.Err() == bufio.ErrTooLong)
	big := bufio.NewScanner(newChunked(long+"\nshort", 4096))
	big.Buffer(make([]byte, 100), len(long)+1)
	for big.Scan() {
		fmt.Println("tok", len(big.Text()))
	}
	fmt.Println(big.Err())

	r := bufio.NewReader(newChunked("héllo\nwörld\nxyzzy\nfinal", 7))
	rn, size, err := r.ReadRune()
	fmt.Println(string(rn), size, err, r.UnreadRune())
	p, err := r.Peek(3)
	fmt.Printf("%q %v %d\n", p, err, r.Buffered())
	line, isPrefix, err := r.ReadLine()
	fmt.Printf("%q %v %v\n", line, isPrefix, err)
	b, err := r.ReadBytes('\n')
	fmt.Printf("%q %v\n", b, err)
	d, err := r.Discard(2)
	fmt.Println(d, err)
	s, err := r.ReadString('\n')
	fmt.Printf("%q %v\n", s, err)
	s, err = r.ReadString('\n')
	fmt.Printf("%q %v %v\n", s, err, err == io.EOF)
	c, err := r.ReadByte()
	fmt.Println(c, err, r.UnreadByte() == bufio.ErrInvalidUnreadByte)

	all, err := io.ReadAll(newChunked(strings.Repeat("ab", 1000), 333))
	fmt.Println(len(all), err, string(all[:4]))
}
