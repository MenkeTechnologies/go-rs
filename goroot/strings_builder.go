// Copyright 2017 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style
// license that can be found in the LICENSE file.

// `strings.Builder` — a growable string built by writing to it.
//
// `strings` is a native host package (its functions are builtins), so this type
// cannot arrive through the ordinary source-package path the way `errors` and
// `io` do. It is instead synthesized into the program the way `fmt.Errorf`'s
// error types are, then qualified under `strings` so the name a program writes
// (`strings.Builder`) is the name the compiler sees.
//
// The standard library's version tracks a `copyCheck` pointer to catch a
// Builder copied after first use, and grows through `unsafe`. Neither is
// observable through the method set, so what is left is the part that is: a
// byte buffer, and the writers over it. `Cap` is deliberately absent — go-rs
// cannot reserve capacity, so answering it would mean answering wrongly, and a
// program that calls it gets a compile error instead.
package strings

type Builder struct {
	buf []byte
}

// String returns the accumulated string.
func (b *Builder) String() string { return string(b.buf) }

// Len returns the number of accumulated bytes.
func (b *Builder) Len() int { return len(b.buf) }

// Reset resets the Builder to be empty.
func (b *Builder) Reset() { b.buf = nil }

// Grow is a no-op: without `Cap` there is nothing that can observe a
// reservation, and Go promises only that a later write will not have to
// reallocate — never that it did not.
func (b *Builder) Grow(n int) {}

// Write appends the contents of p to the Builder's buffer.
func (b *Builder) Write(p []byte) (int, error) {
	b.buf = append(b.buf, p...)
	return len(p), nil
}

// WriteString appends s to the Builder's buffer.
func (b *Builder) WriteString(s string) (int, error) {
	b.buf = append(b.buf, []byte(s)...)
	return len(s), nil
}

// WriteByte appends the byte c to the Builder's buffer.
func (b *Builder) WriteByte(c byte) error {
	b.buf = append(b.buf, c)
	return nil
}

// WriteRune appends the UTF-8 encoding of r to the Builder's buffer, and
// returns the number of *bytes* that took — not 1 for a multi-byte rune.
func (b *Builder) WriteRune(r rune) (int, error) {
	s := string(r)
	b.buf = append(b.buf, []byte(s)...)
	return len(s), nil
}
