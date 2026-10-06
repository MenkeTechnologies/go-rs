package main

// `bufio.Writer` buffers until Flush (or a full buffer), so output written
// straight to os.Stdout in between appears first.

import (
	"bufio"
	"fmt"
	"os"
	"strings"
)

func main() {
	w := bufio.NewWriter(os.Stdout)
	defer w.Flush()
	fmt.Fprintln(w, "hello", 42)
	fmt.Fprintf(w, "%05.1f|%s\n", 3.14159, "x")
	w.WriteString("direct\n")
	w.WriteByte('B')
	w.WriteRune('é')
	w.WriteRune('\n')
	fmt.Println("unbuffered first?", w.Buffered(), w.Available(), w.Size())
	w.Flush()
	small := bufio.NewWriterSize(os.Stdout, 16)
	for i := 0; i < 5; i++ {
		fmt.Fprintf(small, "line %d of a small buffer\n", i)
	}
	small.Write([]byte(strings.Repeat("z", 40) + "\n"))
	small.Flush()
	var sb strings.Builder
	bw := bufio.NewWriter(&sb)
	bw.WriteString("to builder")
	fmt.Println(sb.Len(), bw.Buffered())
	bw.Flush()
	fmt.Println(sb.String())
	same := bufio.NewWriter(w)
	fmt.Println(same == w)
	fmt.Fprint(w, "end", "\n")
}
