// Copyright 2009 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style
// license that can be found in the LICENSE file.

// Package bytes — the `Buffer` a Go program accumulates output in.
//
// `bytes` is not a native host package, so unlike `strings.Builder` this
// arrives through the ordinary source path: it is real Go, parsed and linked
// like `errors` and `io`.
//
// The standard library's Buffer keeps a read offset, a small bootstrap array
// and a `lastRead` field for `UnreadByte`/`UnreadRune`. The read half is what
// those exist for; what a program reaches for when it wants somewhere to write
// is the write half plus `String`/`Bytes`/`Len`, which is what is here. `Cap`
// and `Grow` are deliberately absent for the same reason `strings.Builder.Cap`
// is: go-rs cannot reserve capacity, so answering would mean answering wrongly.
package bytes

// A Buffer is a variable-sized buffer of bytes with Write methods. The
// zero value for Buffer is an empty buffer ready to use.
type Buffer struct {
	buf []byte
}

// NewBufferString returns a new Buffer taking `s` as its initial contents.
func NewBufferString(s string) *Buffer {
	return &Buffer{buf: []byte(s)}
}

// NewBuffer returns a new Buffer taking `b` as its initial contents.
func NewBuffer(b []byte) *Buffer {
	return &Buffer{buf: b}
}

// String returns the contents as a string.
func (b *Buffer) String() string { return string(b.buf) }

// Bytes returns the contents. Writing to the Buffer afterwards may or may not
// be visible through the returned slice, exactly as in Go.
func (b *Buffer) Bytes() []byte { return b.buf }

// Len returns the number of bytes held.
func (b *Buffer) Len() int { return len(b.buf) }

// Reset empties the buffer.
func (b *Buffer) Reset() { b.buf = nil }

// Write appends p, making a Buffer an io.Writer.
func (b *Buffer) Write(p []byte) (int, error) {
	b.buf = append(b.buf, p...)
	return len(p), nil
}

// WriteString appends s, making a Buffer an io.StringWriter.
func (b *Buffer) WriteString(s string) (int, error) {
	b.buf = append(b.buf, []byte(s)...)
	return len(s), nil
}

// WriteByte appends c. The error is always nil, as in Go.
func (b *Buffer) WriteByte(c byte) error {
	b.buf = append(b.buf, c)
	return nil
}

// WriteRune appends the UTF-8 encoding of r, returning the number of *bytes*
// written — not 1 for a multi-byte rune.
func (b *Buffer) WriteRune(r rune) (int, error) {
	s := string(r)
	b.buf = append(b.buf, []byte(s)...)
	return len(s), nil
}
