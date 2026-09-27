// Copyright 2009 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style
// license that can be found in the LICENSE file.

// The `strings` functions that take a function argument.
//
// `strings` is a native host package, and a host builtin cannot call a VM
// closure, so these are written in Go and synthesized into the program the way
// `sort.Slice` is; the compiler routes `strings.Map` and the rest here. Each is
// Go's contract over go-rs's strings, which always hold valid UTF-8, so ranging
// over one yields exactly the runes `utf8.DecodeRuneInString` would. None calls
// another: the linker renames each one on its own.
package strings

// Map returns s with every rune replaced by mapping(r); a negative result drops
// the rune.
func Map(mapping func(rune) rune, s string) string {
	var b []byte
	for _, c := range s {
		r := mapping(c)
		if r >= 0 {
			b = append(b, []byte(string(r))...)
		}
	}
	return string(b)
}

// IndexFunc returns the byte index of the first rune satisfying f, or -1.
func IndexFunc(s string, f func(rune) bool) int {
	for i, r := range s {
		if f(r) {
			return i
		}
	}
	return -1
}

// LastIndexFunc returns the byte index of the last rune satisfying f, or -1.
func LastIndexFunc(s string, f func(rune) bool) int {
	last := -1
	for i, r := range s {
		if f(r) {
			last = i
		}
	}
	return last
}

// ContainsFunc reports whether any rune of s satisfies f.
func ContainsFunc(s string, f func(rune) bool) bool {
	for _, r := range s {
		if f(r) {
			return true
		}
	}
	return false
}

// TrimLeftFunc returns s without its leading runes that satisfy f.
func TrimLeftFunc(s string, f func(rune) bool) string {
	for i, r := range s {
		if !f(r) {
			return s[i:]
		}
	}
	return ""
}

// TrimRightFunc returns s without its trailing runes that satisfy f.
func TrimRightFunc(s string, f func(rune) bool) string {
	end := 0
	for i, r := range s {
		if !f(r) {
			end = i + len(string(r))
		}
	}
	return s[:end]
}

// TrimFunc returns s without its leading and trailing runes that satisfy f.
func TrimFunc(s string, f func(rune) bool) string {
	start, end := -1, 0
	for i, r := range s {
		if !f(r) {
			if start < 0 {
				start = i
			}
			end = i + len(string(r))
		}
	}
	if start < 0 {
		return ""
	}
	return s[start:end]
}

// FieldsFunc splits s around each run of runes satisfying f.
func FieldsFunc(s string, f func(rune) bool) []string {
	out := []string{}
	start := -1
	for i, r := range s {
		if f(r) {
			if start >= 0 {
				out = append(out, s[start:i])
				start = -1
			}
		} else if start < 0 {
			start = i
		}
	}
	if start >= 0 {
		out = append(out, s[start:])
	}
	return out
}
