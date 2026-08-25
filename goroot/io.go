// Copyright 2009 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style
// license that can be found in the LICENSE file.

// Package io provides basic interfaces to I/O primitives.
//
// This is the subset of the standard library's `io` that go-rs can both parse
// and give meaning to. The full package is built on `sync`, `errors` and a
// dozen adapter types whose behaviour is only observable through a real file
// descriptor; what a Go program actually names when it writes portable code is
// the two one-method interfaces below and the helper over them.
//
// They carry no implementation of their own — an interface is a method set, and
// go-rs already dispatches one — so the whole package is the contract.
package io

// Writer is the interface that wraps the basic Write method.
//
// Write writes len(p) bytes from p to the underlying data stream. It returns
// the number of bytes written and an error if fewer were written than asked.
// Implementations must not retain p.
type Writer interface {
	Write(p []byte) (n int, err error)
}

// StringWriter is the interface that wraps the WriteString method.
type StringWriter interface {
	WriteString(s string) (n int, err error)
}

// WriteString writes the contents of s to w, which accepts a slice of bytes. A
// w that implements StringWriter is asked directly, which lets an
// implementation avoid the copy that converting s to []byte would cost.
func WriteString(w Writer, s string) (n int, err error) {
	if sw, ok := w.(StringWriter); ok {
		return sw.WriteString(s)
	}
	return w.Write([]byte(s))
}
