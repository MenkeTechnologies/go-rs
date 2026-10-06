// Copyright 2009 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style
// license that can be found in the LICENSE file.

// The `strconv` functions written in Go rather than as host builtins: the
// unquoting half of quote.go (`Unquote`, `UnquoteChar`, `QuotedPrefix`), which
// returns several results, and the `Append*` family. Like `strings`' Go half
// they are synthesized into a program that names one of them and qualified
// under `strconv` (`pkg::STRCONV_SOURCE`); the `Format*` / `Quote*`
// conversions and the `ErrSyntax` sentinel they reach stay native.
//
// The unquoting bodies are Go's (quote.go) with these adaptations, none
// observable on valid UTF-8, which is all a go-rs string can hold:
//   - `unicode/utf8`, which this package does not import, is replaced by the
//     helpers at the bottom: `decodeRuneInString` reads the first rune with
//     `range`, `validString` is true, `validRune` is utf8.ValidRune's body and
//     `appendRune` appends the rune's encoding.
//   - `index` is bytealg's portable loop.
//
// Each `Append*` is the `Format*` / `Quote*` it pairs with, appended to dst —
// what Go's own bodies produce byte for byte (`formatBits` and `genericFtoa`
// append their digits the same way). Only the capacity a quoting variant
// leaves behind can differ: Go grows dst once for the whole quoted string.
package strconv

import "strconv"

// contains reports whether the string contains the byte c.
func contains(s string, c byte) bool {
	return index(s, c) != -1
}

// index returns the index of the first instance of c in s, or -1 if c is not
// present in s.
func index(s string, c byte) int {
	for i := 0; i < len(s); i++ {
		if s[i] == c {
			return i
		}
	}
	return -1
}

func unhex(b byte) (v rune, ok bool) {
	c := rune(b)
	switch {
	case '0' <= c && c <= '9':
		return c - '0', true
	case 'a' <= c && c <= 'f':
		return c - 'a' + 10, true
	case 'A' <= c && c <= 'F':
		return c - 'A' + 10, true
	}
	return
}

// UnquoteChar decodes the first character or byte in the escaped string
// or character literal represented by the string s.
// It returns four values:
//
//  1. value, the decoded Unicode code point or byte value;
//  2. multibyte, a boolean indicating whether the decoded character requires a multibyte UTF-8 representation;
//  3. tail, the remainder of the string after the character; and
//  4. an error that will be nil if the character is syntactically valid.
//
// The second argument, quote, specifies the type of literal being parsed
// and therefore which escaped quote character is permitted.
// If set to a single quote, it permits the sequence \' and disallows unescaped '.
// If set to a double quote, it permits \" and disallows unescaped ".
// If set to zero, it does not permit either escape and allows both quote characters to appear unescaped.
func UnquoteChar(s string, quote byte) (value rune, multibyte bool, tail string, err error) {
	// easy cases
	if len(s) == 0 {
		err = strconv.ErrSyntax
		return
	}
	switch c := s[0]; {
	case c == quote && (quote == '\'' || quote == '"'):
		err = strconv.ErrSyntax
		return
	case c >= 0x80:
		r, size := decodeRuneInString(s)
		return r, true, s[size:], nil
	case c != '\\':
		return rune(s[0]), false, s[1:], nil
	}

	// hard case: c is backslash
	if len(s) <= 1 {
		err = strconv.ErrSyntax
		return
	}
	c := s[1]
	s = s[2:]

	switch c {
	case 'a':
		value = '\a'
	case 'b':
		value = '\b'
	case 'f':
		value = '\f'
	case 'n':
		value = '\n'
	case 'r':
		value = '\r'
	case 't':
		value = '\t'
	case 'v':
		value = '\v'
	case 'x', 'u', 'U':
		n := 0
		switch c {
		case 'x':
			n = 2
		case 'u':
			n = 4
		case 'U':
			n = 8
		}
		var v rune
		if len(s) < n {
			err = strconv.ErrSyntax
			return
		}
		for j := 0; j < n; j++ {
			x, ok := unhex(s[j])
			if !ok {
				err = strconv.ErrSyntax
				return
			}
			v = v<<4 | x
		}
		s = s[n:]
		if c == 'x' {
			// single-byte string, possibly not UTF-8
			value = v
			break
		}
		if !validRune(v) {
			err = strconv.ErrSyntax
			return
		}
		value = v
		multibyte = true
	case '0', '1', '2', '3', '4', '5', '6', '7':
		v := rune(c) - '0'
		if len(s) < 2 {
			err = strconv.ErrSyntax
			return
		}
		for j := 0; j < 2; j++ { // one digit already; two more
			x := rune(s[j]) - '0'
			if x < 0 || x > 7 {
				err = strconv.ErrSyntax
				return
			}
			v = (v << 3) | x
		}
		s = s[2:]
		if v > 255 {
			err = strconv.ErrSyntax
			return
		}
		value = v
	case '\\':
		value = '\\'
	case '\'', '"':
		if c != quote {
			err = strconv.ErrSyntax
			return
		}
		value = rune(c)
	default:
		err = strconv.ErrSyntax
		return
	}
	tail = s
	return
}

// QuotedPrefix returns the quoted string (as understood by [Unquote]) at the prefix of s.
// If s does not start with a valid quoted string, QuotedPrefix returns an error.
func QuotedPrefix(s string) (string, error) {
	out, _, err := unquote(s, false)
	return out, err
}

// Unquote interprets s as a single-quoted, double-quoted,
// or backquoted Go string literal, returning the string value
// that s quotes.  (If s is single-quoted, it would be a Go
// character literal; Unquote returns the corresponding
// one-character string. For an empty character literal
// Unquote returns the empty string.)
func Unquote(s string) (string, error) {
	out, rem, err := unquote(s, true)
	if len(rem) > 0 {
		return "", strconv.ErrSyntax
	}
	return out, err
}

// unquote parses a quoted string at the start of the input,
// returning the parsed prefix, the remaining suffix, and any parse errors.
// If unescape is true, the parsed prefix is unescaped,
// otherwise the input prefix is provided verbatim.
func unquote(in string, unescape bool) (out, rem string, err error) {
	// Determine the quote form and optimistically find the terminating quote.
	if len(in) < 2 {
		return "", in, strconv.ErrSyntax
	}
	quote := in[0]
	end := index(in[1:], quote)
	if end < 0 {
		return "", in, strconv.ErrSyntax
	}
	end += 2 // position after terminating quote; may be wrong if escape sequences are present

	switch quote {
	case '`':
		switch {
		case !unescape:
			out = in[:end] // include quotes
		case !contains(in[:end], '\r'):
			out = in[len("`") : end-len("`")] // exclude quotes
		default:
			// Carriage return characters ('\r') inside raw string literals
			// are discarded from the raw string value.
			buf := make([]byte, 0, end-len("`")-len("\r")-len("`"))
			for i := len("`"); i < end-len("`"); i++ {
				if in[i] != '\r' {
					buf = append(buf, in[i])
				}
			}
			out = string(buf)
		}
		// NOTE: Prior implementations did not verify that raw strings consist
		// of valid UTF-8 characters and we continue to not verify it as such.
		// The Go specification does not explicitly require valid UTF-8,
		// but only mention that it is implicitly valid for Go source code
		// (which must be valid UTF-8).
		return out, in[end:], nil
	case '"', '\'':
		// Handle quoted strings without any escape sequences.
		if !contains(in[:end], '\\') && !contains(in[:end], '\n') {
			var valid bool
			switch quote {
			case '"':
				valid = validString(in[len(`"`) : end-len(`"`)])
			case '\'':
				r, n := decodeRuneInString(in[len("'") : end-len("'")])
				valid = len("'")+n+len("'") == end && (r != 0xFFFD || n != 1)
			}
			if valid {
				out = in[:end]
				if unescape {
					out = out[1 : end-1] // exclude quotes
				}
				return out, in[end:], nil
			}
		}

		// Handle quoted strings with escape sequences.
		var buf []byte
		in0 := in
		in = in[1:] // skip starting quote
		if unescape {
			buf = make([]byte, 0, 3*end/2) // try to avoid more allocations
		}
		for len(in) > 0 && in[0] != quote {
			// Process the next character,
			// rejecting any unescaped newline characters which are invalid.
			r, multibyte, rem, err := UnquoteChar(in, quote)
			if in[0] == '\n' || err != nil {
				return "", in0, strconv.ErrSyntax
			}
			in = rem

			// Append the character if unescaping the input.
			if unescape {
				if r < 0x80 || !multibyte {
					buf = append(buf, byte(r))
				} else {
					buf = appendRune(buf, r)
				}
			}

			// Single quoted strings must be a single character.
			if quote == '\'' {
				break
			}
		}

		// Verify that the string ends with a terminating quote.
		if !(len(in) > 0 && in[0] == quote) {
			return "", in0, strconv.ErrSyntax
		}
		in = in[1:] // skip terminating quote

		if unescape {
			return string(buf), in, nil
		}
		return in0[:len(in0)-len(in)], in, nil
	default:
		return "", in, strconv.ErrSyntax
	}
}

// AppendBool appends "true" or "false", according to the value of b,
// to dst and returns the extended buffer.
func AppendBool(dst []byte, b bool) []byte {
	if b {
		return append(dst, "true"...)
	}
	return append(dst, "false"...)
}

// AppendInt appends the string form of the integer i,
// as generated by [FormatInt], to dst and returns the extended buffer.
func AppendInt(dst []byte, i int64, base int) []byte {
	return append(dst, strconv.FormatInt(i, base)...)
}

// AppendUint appends the string form of the unsigned integer i,
// as generated by [FormatUint], to dst and returns the extended buffer.
func AppendUint(dst []byte, i uint64, base int) []byte {
	return append(dst, strconv.FormatUint(i, base)...)
}

// AppendFloat appends the string form of the floating-point number f,
// as generated by [FormatFloat], to dst and returns the extended buffer.
func AppendFloat(dst []byte, f float64, fmt byte, prec, bitSize int) []byte {
	return append(dst, strconv.FormatFloat(f, fmt, prec, bitSize)...)
}

// AppendQuote appends a double-quoted Go string literal representing s,
// as generated by [Quote], to dst and returns the extended buffer.
func AppendQuote(dst []byte, s string) []byte {
	return append(dst, strconv.Quote(s)...)
}

// AppendQuoteToASCII appends a double-quoted Go string literal representing s,
// as generated by [QuoteToASCII], to dst and returns the extended buffer.
func AppendQuoteToASCII(dst []byte, s string) []byte {
	return append(dst, strconv.QuoteToASCII(s)...)
}

// AppendQuoteToGraphic appends a double-quoted Go string literal representing s,
// as generated by [QuoteToGraphic], to dst and returns the extended buffer.
func AppendQuoteToGraphic(dst []byte, s string) []byte {
	return append(dst, strconv.QuoteToGraphic(s)...)
}

// AppendQuoteRune appends a single-quoted Go character literal representing the rune,
// as generated by [QuoteRune], to dst and returns the extended buffer.
func AppendQuoteRune(dst []byte, r rune) []byte {
	return append(dst, strconv.QuoteRune(r)...)
}

// AppendQuoteRuneToASCII appends a single-quoted Go character literal representing the rune,
// as generated by [QuoteRuneToASCII], to dst and returns the extended buffer.
func AppendQuoteRuneToASCII(dst []byte, r rune) []byte {
	return append(dst, strconv.QuoteRuneToASCII(r)...)
}

// AppendQuoteRuneToGraphic appends a single-quoted Go character literal representing the rune,
// as generated by [QuoteRuneToGraphic], to dst and returns the extended buffer.
func AppendQuoteRuneToGraphic(dst []byte, r rune) []byte {
	return append(dst, strconv.QuoteRuneToGraphic(r)...)
}

// decodeRuneInString is utf8.DecodeRuneInString on a valid string: the first
// rune and its width, or (RuneError, 0) for "".
func decodeRuneInString(s string) (rune, int) {
	for _, r := range s {
		return r, len(string(r))
	}
	return 0xFFFD, 0
}

// validString is utf8.ValidString, which every go-rs string satisfies.
func validString(s string) bool {
	return true
}

// validRune is utf8.ValidRune: r is a legal Unicode scalar value.
func validRune(r rune) bool {
	switch {
	case 0 <= r && r < 0xD800:
		return true
	case 0xDFFF < r && r <= 0x10FFFF:
		return true
	}
	return false
}

// appendRune is utf8.AppendRune for a valid rune.
func appendRune(p []byte, r rune) []byte {
	return append(p, string(r)...)
}
