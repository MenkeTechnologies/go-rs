// Copyright 2009 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style
// license that can be found in the LICENSE file.

// Files: `os.Open` / `Create` / `OpenFile`, `ReadFile` / `WriteFile`,
// `Remove`, the `*File` they return, and the three standard streams.
//
// `os` is a native host package, so this arrives the way `strings.Builder`
// does: real Go source, synthesized into the program and qualified under `os`.
// The bodies are Go's own `file.go`, `file_posix.go`, `file_unix.go` and
// `error.go` with the layer below them cut away. The standard library's `File`
// wraps a `*file` holding a `poll.FD`, a finalizer and directory state; here the
// descriptor, the name and the closed flag `poll.FD` keeps are the fields, and
// the syscalls `poll.FD` makes are host intrinsics:
//
//	openFd(name, flag, perm) int  — the descriptor, or the negated errno
//	readFd(fd, p) int             — one read(2) into p: the count, or the negated errno
//	writeFd(fd, p) int            — one write(2) of p: the count, or the negated errno
//	closeFd(fd) int               — 0, or the errno
//	fdSize(fd) int                — fstat(2)'s size, or -1
//	unlinkPath(name) int          — 0, or the errno
//	rmdirPath(name) int           — 0, or the errno
//
// An errno becomes a `syscall.Errno` and reaches the program wrapped in a
// `*fs.PathError`, exactly as Go reports it: `open x: no such file or
// directory`, matched by `errors.Is(err, fs.ErrNotExist)` and `os.IsNotExist`.
//
// `os.PathError` and `os.FileMode` are Go type aliases of the `io/fs` types;
// go-rs's linker resolves the two names to `fs.PathError` / `fs.FileMode`
// (`pkg::OS_TYPE_ALIASES`), so `%T` prints `*fs.PathError` as Go does.
package os

import (
	"io"
	"io/fs"
	"syscall"
)

// Portable analogs of some common system call errors.
//
// Errors returned from this package may be tested against these errors
// with [errors.Is].
var (
	// ErrInvalid indicates an invalid argument.
	// Methods on File will return this error when the receiver is nil.
	ErrInvalid = fs.ErrInvalid // "invalid argument"

	ErrPermission = fs.ErrPermission // "permission denied"
	ErrExist      = fs.ErrExist      // "file already exists"
	ErrNotExist   = fs.ErrNotExist   // "file does not exist"
	ErrClosed     = fs.ErrClosed     // "file already closed"
)

// File represents an open file descriptor.
//
// The methods of File are safe for concurrent use.
type File struct {
	fd     int
	name   string
	closed bool // poll.FD's closing state: every later operation reports ErrClosed
}

// Name returns the name of the file as presented to Open.
//
// It is safe to call Name after [Close].
func (f *File) Name() string { return f.name }

// Fd returns the integer Unix file descriptor referencing the open file.
// If f is closed, the file descriptor becomes invalid.
func (f *File) Fd() uintptr {
	if f == nil {
		return ^(uintptr(0))
	}
	return uintptr(f.fd)
}

// NewFile returns a new File with the given file descriptor and
// name. The returned value will be nil if fd is not a valid file
// descriptor.
func NewFile(fd uintptr, name string) *File {
	fdi := int(fd)
	if fdi < 0 {
		return nil
	}
	return &File{fd: fdi, name: name}
}

// Stdin, Stdout, and Stderr are open Files pointing to the standard input,
// standard output, and standard error file descriptors.
//
// Note that the Go runtime writes to standard error for panics and crashes;
// closing Stderr may cause those messages to go elsewhere, perhaps
// to a file opened later.
var (
	Stdin  = NewFile(uintptr(syscall.Stdin), "/dev/stdin")
	Stdout = NewFile(uintptr(syscall.Stdout), "/dev/stdout")
	Stderr = NewFile(uintptr(syscall.Stderr), "/dev/stderr")
)

// Flags to OpenFile wrapping those of the underlying system. Not all
// flags may be implemented on a given system.
var (
	// Exactly one of O_RDONLY, O_WRONLY, or O_RDWR must be specified.
	O_RDONLY int = syscall.O_RDONLY // open the file read-only.
	O_WRONLY int = syscall.O_WRONLY // open the file write-only.
	O_RDWR   int = syscall.O_RDWR   // open the file read-write.
	// The remaining values may be or'ed in to control behavior.
	O_APPEND int = syscall.O_APPEND // append data to the file when writing.
	O_CREATE int = syscall.O_CREAT  // create a new file if none exists.
	O_EXCL   int = syscall.O_EXCL   // used with O_CREATE, file must not exist.
	O_SYNC   int = syscall.O_SYNC   // open for synchronous I/O.
	O_TRUNC  int = syscall.O_TRUNC  // truncate regular writable file when opened.
)

// checkValid checks whether f is valid for use.
// If not, it returns an appropriate error, perhaps incorporating the operation name op.
func (f *File) checkValid(op string) error {
	if f == nil {
		return ErrInvalid
	}
	return nil
}

// wrapErr wraps an error that occurred during an operation on an open file.
// It passes io.EOF through unchanged, otherwise converts
// poll.ErrFileClosing to ErrClosed and wraps the error in a PathError.
func (f *File) wrapErr(op string, err error) error {
	if err == nil || err == io.EOF {
		return err
	}
	return &fs.PathError{Op: op, Path: f.name, Err: err}
}

// Read reads up to len(b) bytes from the File and stores them in b.
// It returns the number of bytes read and any error encountered.
// At end of file, Read returns 0, io.EOF.
func (f *File) Read(b []byte) (n int, err error) {
	if err := f.checkValid("read"); err != nil {
		return 0, err
	}
	n, e := f.read(b)
	return n, f.wrapErr("read", e)
}

// read is poll.FD.Read: ErrClosed once the file is closed, nothing for an
// empty buffer, io.EOF for a read of zero bytes.
func (f *File) read(b []byte) (int, error) {
	if f.closed {
		return 0, ErrClosed
	}
	if len(b) == 0 {
		return 0, nil
	}
	n := readFd(f.fd, b)
	if n < 0 {
		return 0, syscall.Errno(-n)
	}
	if n == 0 {
		return 0, io.EOF
	}
	return n, nil
}

// Write writes len(b) bytes from b to the File.
// It returns the number of bytes written and an error, if any.
// Write returns a non-nil error when n != len(b).
func (f *File) Write(b []byte) (n int, err error) {
	if err := f.checkValid("write"); err != nil {
		return 0, err
	}
	n, e := f.write(b)
	if n < 0 {
		n = 0
	}
	if n != len(b) {
		err = io.ErrShortWrite
	}

	if e != nil {
		err = f.wrapErr("write", e)
	}

	return n, err
}

// write is poll.FD.Write: it calls write(2) until all of b is written or a
// call fails.
func (f *File) write(b []byte) (int, error) {
	if f.closed {
		return 0, ErrClosed
	}
	nn := 0
	for {
		n := writeFd(f.fd, b[nn:])
		if n < 0 {
			return nn, syscall.Errno(-n)
		}
		nn += n
		if nn == len(b) {
			return nn, nil
		}
		if n == 0 {
			return nn, io.ErrUnexpectedEOF
		}
	}
}

// WriteString is like Write, but writes the contents of string s rather than
// a slice of bytes.
func (f *File) WriteString(s string) (n int, err error) {
	return f.Write([]byte(s))
}

// Close closes the [File], rendering it unusable for I/O.
// Close will return an error if it has already been called.
func (f *File) Close() error {
	if f == nil {
		return ErrInvalid
	}
	if f.closed {
		return &fs.PathError{Op: "close", Path: f.name, Err: ErrClosed}
	}
	f.closed = true
	if e := closeFd(f.fd); e != 0 {
		return &fs.PathError{Op: "close", Path: f.name, Err: syscall.Errno(e)}
	}
	return nil
}

// Open opens the named file for reading. If successful, methods on
// the returned file can be used for reading; the associated file
// descriptor has mode O_RDONLY.
// If there is an error, it will be of type [*PathError].
func Open(name string) (*File, error) {
	return OpenFile(name, O_RDONLY, 0)
}

// Create creates or truncates the named file. If the file already exists,
// it is truncated. If the file does not exist, it is created with mode 0o666
// (before umask). If successful, methods on the returned File can
// be used for I/O; the associated file descriptor has mode O_RDWR.
// The directory containing the file must already exist.
// If there is an error, it will be of type *PathError.
func Create(name string) (*File, error) {
	return OpenFile(name, O_RDWR|O_CREATE|O_TRUNC, 0666)
}

// OpenFile is the generalized open call; most users will use Open
// or Create instead. It opens the named file with specified flag
// ([O_RDONLY] etc.). If the file does not exist, and the [O_CREATE] flag
// is passed, it is created with mode perm (before umask);
// the containing directory must exist. If successful,
// methods on the returned File can be used for I/O.
// If there is an error, it will be of type *PathError.
func OpenFile(name string, flag int, perm fs.FileMode) (*File, error) {
	fd := openFd(name, flag, int(perm.Perm()))
	if fd < 0 {
		return nil, &fs.PathError{Op: "open", Path: name, Err: syscall.Errno(-fd)}
	}
	return NewFile(uintptr(fd), name), nil
}

// ReadFile reads the named file and returns the contents.
// A successful call returns err == nil, not err == EOF.
// Because ReadFile reads the whole file, it does not treat an EOF from Read
// as an error to be reported.
func ReadFile(name string) ([]byte, error) {
	f, err := Open(name)
	if err != nil {
		return nil, err
	}
	defer f.Close()

	return readFileContents(statOrZero(f), f.Read)
}

// statOrZero is the size `f.Stat()` reports, or 0 when the stat fails.
func statOrZero(f *File) int64 {
	if size := fdSize(f.fd); size >= 0 {
		return int64(size)
	}
	return 0
}

// readFileContents reads the contents of a file using the provided read function
// (*os.File.Read, except in tests) one or more times, until an error is seen.
//
// The provided size is the stat size of the file, which might be 0 for a
// /proc-like file that doesn't report a size.
func readFileContents(statSize int64, read func([]byte) (int, error)) ([]byte, error) {
	zeroSize := statSize == 0

	// Figure out how big to make the initial slice. For files with known size
	// that fit in memory, use that size + 1. Otherwise, use a small buffer and
	// we'll grow.
	var size int
	if int64(int(statSize)) == statSize {
		size = int(statSize)
	}
	size++ // one byte for final read at EOF

	const minBuf = 512
	// If a file claims a small size, read at least 512 bytes. In particular,
	// files in Linux's /proc claim size 0 but then do not work right if read in
	// small pieces, so an initial read of 1 byte would not work correctly.
	if size < minBuf {
		size = minBuf
	}

	data := make([]byte, 0, size)
	for {
		n, err := read(data[len(data):cap(data)])
		data = data[:len(data)+n]
		if err != nil {
			if err == io.EOF {
				err = nil
			}
			return data, err
		}

		// If we're either out of capacity or if the file was a /proc-like zero
		// sized file, grow the buffer. Per Issue 72080, we always want to issue
		// Read calls on zero-length files with a non-tiny buffer size.
		capRemain := cap(data) - len(data)
		if capRemain == 0 || (zeroSize && capRemain < minBuf) {
			data = grow(data, minBuf)
		}
	}
}

// grow is `slices.Grow` for a []byte — the body of Go's generic one —
// so linking `os` does not link all of `slices`.
func grow(s []byte, n int) []byte {
	if n -= cap(s) - len(s); n > 0 {
		s = append(s[:cap(s)], make([]byte, n)...)[:len(s)]
	}
	return s
}

// WriteFile writes data to the named file, creating it if necessary.
// If the file does not exist, WriteFile creates it with permissions perm (before umask);
// otherwise WriteFile truncates it before writing, without changing permissions.
// Since WriteFile requires multiple system calls to complete, a failure mid-operation
// can leave the file in a partially written state.
func WriteFile(name string, data []byte, perm fs.FileMode) error {
	f, err := OpenFile(name, O_WRONLY|O_CREATE|O_TRUNC, perm)
	if err != nil {
		return err
	}
	_, err = f.Write(data)
	if err1 := f.Close(); err1 != nil && err == nil {
		err = err1
	}
	return err
}

// Remove removes the named file or (empty) directory.
// If there is an error, it will be of type [*PathError].
func Remove(name string) error {
	// System call interface forces us to know
	// whether name is a file or directory.
	// Try both: it is cheaper on average than
	// doing a Stat plus the right one.
	e := unlinkPath(name)
	if e == 0 {
		return nil
	}
	e1 := rmdirPath(name)
	if e1 == 0 {
		return nil
	}

	// Both failed: figure out which error to return.
	// OS X and Linux differ on whether unlink(dir)
	// returns EISDIR, so can't use that. However,
	// both agree that rmdir(file) returns ENOTDIR,
	// so we can use that to decide which error is real.
	// Rmdir might also return ENOTDIR if given a bad
	// file path, like /etc/passwd/foo, but in that case,
	// both errors will be ENOTDIR, so it's okay to
	// use the error from unlink.
	if syscall.Errno(e1) != syscall.ENOTDIR {
		e = e1
	}
	return &fs.PathError{Op: "remove", Path: name, Err: syscall.Errno(e)}
}

// IsExist returns a boolean indicating whether its argument is known to report
// that a file or directory already exists. It is satisfied by [ErrExist] as
// well as some syscall errors.
//
// This function predates [errors.Is]. It only supports errors returned by
// the os package. New code should use errors.Is(err, fs.ErrExist).
func IsExist(err error) bool {
	return underlyingErrorIs(err, ErrExist)
}

// IsNotExist returns a boolean indicating whether its argument is known to
// report that a file or directory does not exist. It is satisfied by
// [ErrNotExist] as well as some syscall errors.
//
// This function predates [errors.Is]. It only supports errors returned by
// the os package. New code should use errors.Is(err, fs.ErrNotExist).
func IsNotExist(err error) bool {
	return underlyingErrorIs(err, ErrNotExist)
}

// IsPermission returns a boolean indicating whether its argument is known to
// report that permission is denied. It is satisfied by [ErrPermission] as well
// as some syscall errors.
//
// This function predates [errors.Is]. It only supports errors returned by
// the os package. New code should use errors.Is(err, fs.ErrPermission).
func IsPermission(err error) bool {
	return underlyingErrorIs(err, ErrPermission)
}

func underlyingErrorIs(err, target error) bool {
	// Note that this function is not errors.Is:
	// underlyingError only unwraps the specific error-wrapping types
	// that it historically did, not all errors implementing Unwrap().
	err = underlyingError(err)
	if err == target {
		return true
	}
	// To preserve prior behavior, only examine syscall errors.
	e, ok := err.(syscall.Errno)
	return ok && e.Is(target)
}

// underlyingError returns the underlying error for known os error types.
func underlyingError(err error) error {
	switch err := err.(type) {
	case *fs.PathError:
		return err.Err
	}
	return err
}
