// Copyright 2009 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style
// license that can be found in the LICENSE file.

// `os.Stdin` / `os.Stdout` / `os.Stderr` — the three files a Go program reads
// and writes without opening anything.
//
// `os` is a native host package, so this arrives the way `strings.Builder`
// does: real Go source, synthesized into the program and qualified under `os`.
// The standard library's `File` wraps a `*file` with a finalizer, a poll.FD and
// a name; none of that is reachable from the three values below, so what is left
// is the descriptor, the method that makes a `*File` an `io.Writer`, and the
// one that makes it an `io.Reader`.
//
// `writeFd` and `readFd` are host intrinsics — the actual write and read
// syscalls, which no Go source here can name. Only `Stdin`, `Stdout` and
// `Stderr` are constructed, so the only descriptors they ever see are 0, 1 and 2.
package os

import "io"

type File struct {
	fd int
}

// Fd returns the file's descriptor number.
func (f *File) Fd() int { return f.fd }

// Write writes len(p) bytes to the file. It never reports a short write: the
// underlying intrinsic writes the whole slice or the process has already failed.
func (f *File) Write(p []byte) (int, error) {
	writeFd(f.fd, string(p))
	return len(p), nil
}

// WriteString writes s to the file, avoiding the []byte round trip.
func (f *File) WriteString(s string) (int, error) {
	writeFd(f.fd, s)
	return len(s), nil
}

// Read reads up to len(p) bytes into p. It returns the number of bytes read
// and io.EOF once the descriptor has no more data.
func (f *File) Read(p []byte) (int, error) {
	if len(p) == 0 {
		return 0, nil
	}
	n := copy(p, readFd(f.fd, len(p)))
	if n == 0 {
		return 0, io.EOF
	}
	return n, nil
}

var Stdin = &File{0}

var Stdout = &File{1}

var Stderr = &File{2}
