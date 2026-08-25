// Copyright 2009 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style
// license that can be found in the LICENSE file.

// `os.Stdout` / `os.Stderr` — the two files a Go program writes to without
// opening anything.
//
// `os` is a native host package, so this arrives the way `strings.Builder`
// does: real Go source, synthesized into the program and qualified under `os`.
// The standard library's `File` wraps a `*file` with a finalizer, a poll.FD and
// a name; none of that is reachable from the two values below, so what is left
// is the descriptor and the one method that makes a `*File` an `io.Writer`.
//
// `writeFd` is a host intrinsic — the actual write syscall, which no Go source
// here can name. Only `Stdout` and `Stderr` are constructed, so the only
// descriptors it ever sees are 1 and 2.
package os

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

var Stdout = &File{1}

var Stderr = &File{2}
