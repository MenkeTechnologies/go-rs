// Copyright 2009 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style
// license that can be found in the LICENSE file.

// The `strings` functions written in Go rather than as host builtins: the ones
// returning several results (`Cut`, `CutPrefix`, `CutSuffix`), the
// `SplitAfter` pair Go builds on its own `genSplit`, `LastIndexAny`, and the
// `Replacer` type. Like `sort`'s Go half they are synthesized into a program
// that imports `strings` and qualified under it (`pkg::STRINGS_SOURCE`); the
// `strings.Index` / `Count` / `HasPrefix` they call stay the native builtins.
//
// The bodies are Go's (strings.go, stringslite, replace.go) with two
// adaptations, neither observable on valid UTF-8, which is all a go-rs string
// can hold:
//   - `explode` and `LastIndexAny` step over runes with `range` / a lead-byte
//     width instead of `unicode/utf8`, which this package does not import.
//   - `Replacer` keeps its pairs and matches them in argument order at each
//     position, where Go first builds a byte table, a single-string finder or
//     a trie. Go's every algorithm picks, at each position, the earliest pair
//     whose old string is a prefix there, and skips an empty match right after
//     another — the rule `genericReplacer.WriteString` states — so the result
//     is the same. A position advances by a rune rather than a byte, which
//     differs from Go only for an empty old string inside a multi-byte rune.
package strings

import "strings"

// Cut slices s around the first instance of sep, returning the text before
// and after sep. The found result reports whether sep appears in s. If sep
// does not appear in s, cut returns s, "", false.
func Cut(s, sep string) (before, after string, found bool) {
	if i := strings.Index(s, sep); i >= 0 {
		return s[:i], s[i+len(sep):], true
	}
	return s, "", false
}

// CutPrefix returns s without the provided leading prefix string and reports
// whether it found the prefix. If s doesn't start with prefix, CutPrefix
// returns s, false. If prefix is the empty string, CutPrefix returns s, true.
func CutPrefix(s, prefix string) (after string, found bool) {
	if !strings.HasPrefix(s, prefix) {
		return s, false
	}
	return s[len(prefix):], true
}

// CutSuffix returns s without the provided ending suffix string and reports
// whether it found the suffix. If s doesn't end with suffix, CutSuffix returns
// s, false. If suffix is the empty string, CutSuffix returns s, true.
func CutSuffix(s, suffix string) (before string, found bool) {
	if !strings.HasSuffix(s, suffix) {
		return s, false
	}
	return s[:len(s)-len(suffix)], true
}

// SplitAfter slices s into all substrings after each instance of sep and
// returns a slice of those substrings.
func SplitAfter(s, sep string) []string {
	return genSplit(s, sep, len(sep), -1)
}

// SplitAfterN slices s into substrings after each instance of sep and returns
// a slice of those substrings. The count determines the number of substrings
// to return: n > 0 at most n substrings, n == 0 the result is nil, n < 0 all.
func SplitAfterN(s, sep string, n int) []string {
	return genSplit(s, sep, len(sep), n)
}

// genSplit splits s around each instance of sep, including sepSave bytes of
// sep in the subarrays.
func genSplit(s, sep string, sepSave, n int) []string {
	if n == 0 {
		return nil
	}
	if sep == "" {
		return explode(s, n)
	}
	if n < 0 {
		n = strings.Count(s, sep) + 1
	}

	n = min(n, len(s)+1)
	a := make([]string, n)
	n--
	i := 0
	for i < n {
		m := strings.Index(s, sep)
		if m < 0 {
			break
		}
		a[i] = s[:m+sepSave]
		s = s[m+len(sep):]
		i++
	}
	a[i] = s
	return a[:i+1]
}

// explode splits s into a slice of UTF-8 strings, one string per rune, up to
// a maximum of n (n < 0 means no limit).
func explode(s string, n int) []string {
	var runes []string
	for _, r := range s {
		runes = append(runes, string(r))
	}
	l := len(runes)
	if n < 0 || n > l {
		n = l
	}
	a := make([]string, n)
	off := 0
	for i := 0; i < n-1; i++ {
		a[i] = runes[i]
		off += len(runes[i])
	}
	if n > 0 {
		a[n-1] = s[off:]
	}
	return a
}

// LastIndexAny returns the index of the last instance of any Unicode code
// point from chars in s, or -1 if no Unicode code point from chars is present
// in s.
func LastIndexAny(s, chars string) int {
	if chars == "" {
		return -1
	}
	last := -1
	for i, r := range s {
		if strings.ContainsRune(chars, r) {
			last = i
		}
	}
	return last
}

// Replacer replaces a list of strings with replacements.
type Replacer struct {
	oldnew []string
}

// NewReplacer returns a new Replacer from a list of old, new string pairs.
// Replacements are performed in the order they appear in the target string,
// without overlapping matches. The old string comparisons are done in
// argument order.
//
// NewReplacer panics if given an odd number of arguments.
func NewReplacer(oldnew ...string) *Replacer {
	if len(oldnew)%2 == 1 {
		panic("strings.NewReplacer: odd argument count")
	}
	return &Replacer{oldnew: append([]string(nil), oldnew...)}
}

// lookup returns the replacement for the earliest pair whose old string is a
// prefix of s, ignoring an empty old string when ignoreEmpty is set.
func (r *Replacer) lookup(s string, ignoreEmpty bool) (val string, keylen int, found bool) {
	for k := 0; k < len(r.oldnew); k += 2 {
		old := r.oldnew[k]
		if old == "" && ignoreEmpty {
			continue
		}
		if strings.HasPrefix(s, old) {
			return r.oldnew[k+1], len(old), true
		}
	}
	return "", 0, false
}

// Replace returns a copy of s with all replacements performed.
func (r *Replacer) Replace(s string) string {
	var b strings.Builder
	last := 0
	prevMatchEmpty := false
	for i := 0; i <= len(s); {
		// Ignore the empty match iff the previous loop found the empty match.
		val, keylen, match := r.lookup(s[i:], prevMatchEmpty)
		prevMatchEmpty = match && keylen == 0
		if match {
			b.WriteString(s[last:i])
			b.WriteString(val)
			i += keylen
			last = i
			continue
		}
		i += runeWidth(s, i)
	}
	if last != len(s) {
		b.WriteString(s[last:])
	}
	return b.String()
}

// runeWidth is the byte length of the rune starting at s[i] (1 at the end).
func runeWidth(s string, i int) int {
	if i >= len(s) {
		return 1
	}
	c := s[i]
	switch {
	case c < 0xC0:
		return 1
	case c < 0xE0:
		return 2
	case c < 0xF0:
		return 3
	}
	return 4
}
