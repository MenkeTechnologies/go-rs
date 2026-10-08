// Copyright 2009 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style
// license that can be found in the LICENSE file.

// Package bytes — the `Buffer` a Go program accumulates output in, and the
// package-level functions over byte slices (`bytes.go`, below).
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

import "strings"

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

// ---- bytes.go ----
//
// The package-level functions are Go's `bytes.go` with one adaptation: where
// Go calls `internal/bytealg`, `unicode` or `unicode/utf8`, these call the
// native `strings` builtin on `string(s)` — the same index, count or case
// mapping, since a byte offset into `s` is a byte offset into `string(s)` — and
// a result that Go slices out of `s` is still sliced out of `s`, so it aliases
// the argument exactly as Go's does. The `unicode` tables are not imported:
// they are the expensive part of a program that names them, and `strings`
// already carries them. Rune widths are read from the lead byte, as in
// `goroot/strings_source.go`; the two agree on valid UTF-8.

// Equal reports whether a and b
// are the same length and contain the same bytes.
// A nil argument is equivalent to an empty slice.
func Equal(a, b []byte) bool {
	return string(a) == string(b)
}

// Compare returns an integer comparing two byte slices lexicographically.
// The result will be 0 if a == b, -1 if a < b, and +1 if a > b.
// A nil argument is equivalent to an empty slice.
func Compare(a, b []byte) int {
	return strings.Compare(string(a), string(b))
}

// leadWidth is the byte length of the UTF-8 sequence starting s, clamped to
// len(s): what `utf8.DecodeRune` reports as the size on valid input.
func leadWidth(s []byte) int {
	n := 1
	switch c := s[0]; {
	case c >= 0xF0:
		n = 4
	case c >= 0xE0:
		n = 3
	case c >= 0xC0:
		n = 2
	}
	return min(n, len(s))
}

// explode splits s into a slice of UTF-8 sequences, one per Unicode code point (still slices of bytes),
// up to a maximum of n byte slices. Invalid UTF-8 sequences are chopped into individual bytes.
func explode(s []byte, n int) [][]byte {
	if n <= 0 || n > len(s) {
		n = len(s)
	}
	a := make([][]byte, n)
	var size int
	na := 0
	for len(s) > 0 {
		if na+1 >= n {
			a[na] = s
			na++
			break
		}
		size = leadWidth(s)
		a[na] = s[0:size:size]
		s = s[size:]
		na++
	}
	return a[0:na]
}

// Count counts the number of non-overlapping instances of sep in s.
// If sep is an empty slice, Count returns 1 + the number of UTF-8-encoded code points in s.
func Count(s, sep []byte) int {
	return strings.Count(string(s), string(sep))
}

// Contains reports whether subslice is within b.
func Contains(b, subslice []byte) bool {
	return Index(b, subslice) != -1
}

// ContainsAny reports whether any of the UTF-8-encoded code points in chars are within b.
func ContainsAny(b []byte, chars string) bool {
	return IndexAny(b, chars) >= 0
}

// ContainsRune reports whether the rune is contained in the UTF-8-encoded byte slice b.
func ContainsRune(b []byte, r rune) bool {
	return IndexRune(b, r) >= 0
}

// ContainsFunc reports whether any of the UTF-8-encoded code points r within b satisfy f(r).
// It stops as soon as a call to f returns true.
func ContainsFunc(b []byte, f func(rune) bool) bool {
	return IndexFunc(b, f) >= 0
}

// Index returns the index of the first instance of sep in s, or -1 if sep is not present in s.
func Index(s, sep []byte) int {
	return strings.Index(string(s), string(sep))
}

// IndexByte returns the index of the first instance of c in b, or -1 if c is not present in b.
func IndexByte(b []byte, c byte) int {
	return strings.IndexByte(string(b), c)
}

// IndexRune interprets s as a sequence of UTF-8-encoded code points.
// It returns the byte index of the first occurrence in s of the given rune.
// It returns -1 if rune is not present in s.
func IndexRune(s []byte, r rune) int {
	return strings.IndexRune(string(s), r)
}

// IndexAny interprets s as a sequence of UTF-8-encoded Unicode code points.
// It returns the byte index of the first occurrence in s of any of the Unicode
// code points in chars. It returns -1 if chars is empty or if there is no code
// point in common.
func IndexAny(s []byte, chars string) int {
	return strings.IndexAny(string(s), chars)
}

// IndexFunc interprets s as a sequence of UTF-8-encoded code points.
// It returns the byte index in s of the first Unicode
// code point satisfying f(c), or -1 if none do.
func IndexFunc(s []byte, f func(r rune) bool) int {
	for i, r := range string(s) {
		if f(r) {
			return i
		}
	}
	return -1
}

// LastIndex returns the index of the last instance of sep in s, or -1 if sep is not present in s.
func LastIndex(s, sep []byte) int {
	return strings.LastIndex(string(s), string(sep))
}

// LastIndexByte returns the index of the last instance of c in s, or -1 if c is not present in s.
func LastIndexByte(s []byte, c byte) int {
	return strings.LastIndexByte(string(s), c)
}

// Generic split: splits after each instance of sep,
// including sepSave bytes of sep in the subslices.
func genSplit(s, sep []byte, sepSave, n int) [][]byte {
	if n == 0 {
		return nil
	}
	if len(sep) == 0 {
		return explode(s, n)
	}
	if n < 0 {
		n = Count(s, sep) + 1
	}
	n = min(n, len(s)+1)

	a := make([][]byte, n)
	n--
	i := 0
	for i < n {
		m := Index(s, sep)
		if m < 0 {
			break
		}
		a[i] = s[: m+sepSave : m+sepSave]
		s = s[m+len(sep):]
		i++
	}
	a[i] = s
	return a[:i+1]
}

// SplitN slices s into subslices separated by sep and returns a slice of
// the subslices between those separators.
func SplitN(s, sep []byte, n int) [][]byte { return genSplit(s, sep, 0, n) }

// SplitAfterN slices s into subslices after each instance of sep and
// returns a slice of those subslices.
func SplitAfterN(s, sep []byte, n int) [][]byte {
	return genSplit(s, sep, len(sep), n)
}

// Split slices s into all subslices separated by sep and returns a slice of
// the subslices between those separators.
func Split(s, sep []byte) [][]byte { return genSplit(s, sep, 0, -1) }

// SplitAfter slices s into all subslices after each instance of sep and
// returns a slice of those subslices.
func SplitAfter(s, sep []byte) [][]byte {
	return genSplit(s, sep, len(sep), -1)
}

// Fields splits the slice s around each instance of one or more consecutive
// white space characters, as defined by unicode.IsSpace, returning a slice of
// subslices of s or an empty slice if s contains only white space.
//
// Each field `strings.Fields` finds is located in s by searching forward from
// the end of the previous one: everything in between is white space, and a
// field begins with a non-space rune, so the first match is the field itself.
func Fields(s []byte) [][]byte {
	fs := strings.Fields(string(s))
	a := make([][]byte, len(fs))
	pos := 0
	for i, f := range fs {
		start := pos + strings.Index(string(s[pos:]), f)
		end := start + len(f)
		a[i] = s[start:end:end]
		pos = end
	}
	return a
}

// Join concatenates the elements of s to create a new byte slice. The separator
// sep is placed between elements in the resulting slice.
func Join(s [][]byte, sep []byte) []byte {
	if len(s) == 0 {
		return []byte{}
	}
	if len(s) == 1 {
		// Just return a copy.
		return append([]byte(nil), s[0]...)
	}

	n := len(sep) * (len(s) - 1)
	for _, v := range s {
		n += len(v)
	}

	b := make([]byte, n)
	bp := copy(b, s[0])
	for _, v := range s[1:] {
		bp += copy(b[bp:], sep)
		bp += copy(b[bp:], v)
	}
	return b
}

// HasPrefix reports whether the byte slice s begins with prefix.
func HasPrefix(s, prefix []byte) bool {
	return len(s) >= len(prefix) && Equal(s[:len(prefix)], prefix)
}

// HasSuffix reports whether the byte slice s ends with suffix.
func HasSuffix(s, suffix []byte) bool {
	return len(s) >= len(suffix) && Equal(s[len(s)-len(suffix):], suffix)
}

// Map returns a copy of the byte slice s with all its characters modified
// according to the mapping function. If mapping returns a negative value, the character is
// dropped from the byte slice with no replacement.
func Map(mapping func(r rune) rune, s []byte) []byte {
	b := make([]byte, 0, len(s))
	for _, r := range string(s) {
		r = mapping(r)
		if r >= 0 {
			b = append(b, string(r)...)
		}
	}
	return b
}

// Repeat returns a new byte slice consisting of count copies of b.
//
// It panics if count is negative.
func Repeat(b []byte, count int) []byte {
	if count == 0 {
		return []byte{}
	}
	if count < 0 {
		panic("bytes: negative Repeat count")
	}
	n := len(b) * count
	if len(b) == 0 {
		return []byte{}
	}
	nb := make([]byte, n)
	bp := copy(nb, b)
	for bp < n {
		bp += copy(nb[bp:], nb[:bp])
	}
	return nb
}

// ToUpper returns a copy of the byte slice s with all Unicode letters mapped to
// their upper case.
func ToUpper(s []byte) []byte {
	isASCII, hasLower := true, false
	for i := 0; i < len(s); i++ {
		c := s[i]
		if c >= 0x80 {
			isASCII = false
			break
		}
		hasLower = hasLower || ('a' <= c && c <= 'z')
	}

	if isASCII { // optimize for ASCII-only byte slices.
		if !hasLower {
			// Just return a copy.
			return append([]byte(""), s...)
		}
		b := make([]byte, len(s))
		for i := 0; i < len(s); i++ {
			c := s[i]
			if 'a' <= c && c <= 'z' {
				c -= 'a' - 'A'
			}
			b[i] = c
		}
		return b
	}
	return []byte(strings.ToUpper(string(s)))
}

// ToLower returns a copy of the byte slice s with all Unicode letters mapped to
// their lower case.
func ToLower(s []byte) []byte {
	isASCII, hasUpper := true, false
	for i := 0; i < len(s); i++ {
		c := s[i]
		if c >= 0x80 {
			isASCII = false
			break
		}
		hasUpper = hasUpper || ('A' <= c && c <= 'Z')
	}

	if isASCII { // optimize for ASCII-only byte slices.
		if !hasUpper {
			return append([]byte(""), s...)
		}
		b := make([]byte, len(s))
		for i := 0; i < len(s); i++ {
			c := s[i]
			if 'A' <= c && c <= 'Z' {
				c += 'a' - 'A'
			}
			b[i] = c
		}
		return b
	}
	return []byte(strings.ToLower(string(s)))
}

// ToTitle treats s as UTF-8-encoded bytes and returns a copy with all the Unicode letters mapped to their title case.
func ToTitle(s []byte) []byte { return []byte(strings.ToTitle(string(s))) }

// TrimPrefix returns s without the provided leading prefix string.
// If s doesn't start with prefix, s is returned unchanged.
func TrimPrefix(s, prefix []byte) []byte {
	if HasPrefix(s, prefix) {
		return s[len(prefix):]
	}
	return s
}

// TrimSuffix returns s without the provided trailing suffix string.
// If s doesn't end with suffix, s is returned unchanged.
func TrimSuffix(s, suffix []byte) []byte {
	if HasSuffix(s, suffix) {
		return s[:len(s)-len(suffix)]
	}
	return s
}

// Trim returns a subslice of s by slicing off all leading and
// trailing UTF-8-encoded code points contained in cutset.
func Trim(s []byte, cutset string) []byte {
	if len(s) == 0 {
		// This is what we've historically done.
		return nil
	}
	if cutset == "" {
		return s
	}
	return TrimLeft(TrimRight(s, cutset), cutset)
}

// TrimLeft returns a subslice of s by slicing off all leading
// UTF-8-encoded code points contained in cutset.
func TrimLeft(s []byte, cutset string) []byte {
	if len(s) == 0 {
		// This is what we've historically done.
		return nil
	}
	if cutset == "" {
		return s
	}
	rest := strings.TrimLeft(string(s), cutset)
	if len(rest) == 0 {
		// This is what we've historically done.
		return nil
	}
	return s[len(s)-len(rest):]
}

// TrimRight returns a subslice of s by slicing off all trailing
// UTF-8-encoded code points that are contained in cutset.
func TrimRight(s []byte, cutset string) []byte {
	if len(s) == 0 || cutset == "" {
		return s
	}
	return s[:len(strings.TrimRight(string(s), cutset))]
}

// TrimSpace returns a subslice of s by slicing off all leading and
// trailing white space, as defined by Unicode. A slice that is all white
// space trims to nil, as Go's does.
func TrimSpace(s []byte) []byte {
	t := strings.TrimSpace(string(s))
	if len(t) == 0 {
		return nil
	}
	// Everything before t is white space and t starts with a non-space rune,
	// so its first occurrence is where it was cut from.
	lo := strings.Index(string(s), t)
	return s[lo : lo+len(t)]
}

// Runes interprets s as a sequence of UTF-8-encoded code points.
// It returns a slice of runes (Unicode code points) equivalent to s.
func Runes(s []byte) []rune {
	return []rune(string(s))
}

// Replace returns a copy of the slice s with the first n
// non-overlapping instances of old replaced by new.
// If old is empty, it matches at the beginning of the slice
// and after each UTF-8 sequence, yielding up to k+1 replacements
// for a k-rune slice.
// If n < 0, there is no limit on the number of replacements.
func Replace(s, old, new []byte, n int) []byte {
	m := 0
	if n != 0 {
		// Compute number of replacements.
		m = Count(s, old)
	}
	if m == 0 {
		// Just return a copy.
		return append([]byte(nil), s...)
	}
	if n < 0 || m < n {
		n = m
	}

	// Apply replacements to buffer.
	t := make([]byte, len(s)+n*(len(new)-len(old)))
	w := 0
	start := 0
	if len(old) > 0 {
		for k := 0; k < n; k++ {
			j := start + Index(s[start:], old)
			w += copy(t[w:], s[start:j])
			w += copy(t[w:], new)
			start = j + len(old)
		}
	} else { // len(old) == 0
		w += copy(t[w:], new)
		for k := 0; k < n-1; k++ {
			j := start + leadWidth(s[start:])
			w += copy(t[w:], s[start:j])
			w += copy(t[w:], new)
			start = j
		}
	}
	w += copy(t[w:], s[start:])
	return t[0:w]
}

// ReplaceAll returns a copy of the slice s with all
// non-overlapping instances of old replaced by new.
func ReplaceAll(s, old, new []byte) []byte {
	return Replace(s, old, new, -1)
}

// EqualFold reports whether s and t, interpreted as UTF-8 strings,
// are equal under simple Unicode case-folding, which is a more general
// form of case-insensitivity.
func EqualFold(s, t []byte) bool {
	return strings.EqualFold(string(s), string(t))
}

// Cut slices s around the first instance of sep,
// returning the text before and after sep.
// The found result reports whether sep appears in s.
// If sep does not appear in s, cut returns s, nil, false.
//
// Cut returns slices of the original slice s, not copies.
func Cut(s, sep []byte) (before, after []byte, found bool) {
	if i := Index(s, sep); i >= 0 {
		return s[:i], s[i+len(sep):], true
	}
	return s, nil, false
}

// Clone returns a copy of b[:len(b)].
// The result may have additional unused capacity.
// Clone(nil) returns nil.
func Clone(b []byte) []byte {
	if b == nil {
		return nil
	}
	return append([]byte{}, b...)
}

// CutPrefix returns s without the provided leading prefix byte slice
// and reports whether it found the prefix.
// If s doesn't start with prefix, CutPrefix returns s, false.
// If prefix is the empty byte slice, CutPrefix returns s, true.
func CutPrefix(s, prefix []byte) (after []byte, found bool) {
	if !HasPrefix(s, prefix) {
		return s, false
	}
	return s[len(prefix):], true
}

// CutSuffix returns s without the provided ending suffix byte slice
// and reports whether it found the suffix.
// If s doesn't end with suffix, CutSuffix returns s, false.
// If suffix is the empty byte slice, CutSuffix returns s, true.
func CutSuffix(s, suffix []byte) (before []byte, found bool) {
	if !HasSuffix(s, suffix) {
		return s, false
	}
	return s[:len(s)-len(suffix)], true
}
