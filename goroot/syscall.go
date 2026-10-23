// Copyright 2009 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style
// license that can be found in the LICENSE file.

// Package syscall contains an interface to the low-level operating system
// primitives.
//
// This is the sliver of the standard library's `syscall` that `os` lets a
// program observe: the three standard descriptor numbers, and `Errno` — the
// value in a `*fs.PathError`'s `Err` after a failed open, read, write, close or
// remove, whose `Error()` is the C library's text and whose `Is` lets
// `errors.Is(err, fs.ErrNotExist)` match an `ENOENT`.
//
// The system calls themselves are not here. go-rs's `os` performs them through
// host intrinsics that report an errno number, and that number is what becomes
// an `Errno`.
//
// `strerror` and `sysConst` are host intrinsics. Go's `Error()` reads a
// per-platform table generated from the C library's messages
// (`zerrors_linux_amd64.go`, `zerrors_darwin_arm64.go`); `strerror` asks the
// running platform's C library for the same text instead of carrying every
// table. The errno numbers and `open(2)` flags that differ between Linux and Darwin
// are likewise taken from the host, so a comparison such as `e == ENOTEMPTY` holds on
// whichever system the program runs.
package syscall

import (
	"errors"
	"internal/oserror"
	"strconv"
)

const (
	Stdin  = 0
	Stdout = 1
	Stderr = 2
)

// An Errno is an unsigned number describing an error condition.
// It implements the error interface. The zero Errno is by convention
// a non-error, so code to convert from Errno to error should use:
//
//	err = nil
//	if errno != 0 {
//		err = errno
//	}
//
// Errno values can be tested against error values using [errors.Is].
// For example:
//
//	_, _, err := syscall.Syscall(...)
//	if errors.Is(err, fs.ErrNotExist) ...
type Errno uintptr

func (e Errno) Error() string {
	s := strerror(int(e))
	if s != "" {
		return s
	}
	return "errno " + strconv.Itoa(int(e))
}

func (e Errno) Is(target error) bool {
	switch target {
	case oserror.ErrPermission:
		return e == EACCES || e == EPERM
	case oserror.ErrExist:
		return e == EEXIST || e == ENOTEMPTY
	case oserror.ErrNotExist:
		return e == ENOENT
	case errors.ErrUnsupported:
		return e == ENOSYS || e == ENOTSUP || e == EOPNOTSUPP
	}
	return false
}

func (e Errno) Temporary() bool {
	return e == EINTR || e == EMFILE || e == ENFILE || e.Timeout()
}

func (e Errno) Timeout() bool {
	return e == EAGAIN || e == EWOULDBLOCK || e == ETIMEDOUT
}

// The errno values that are the same number on every Unix go-rs builds for.
const (
	EPERM   = Errno(0x1)
	ENOENT  = Errno(0x2)
	EINTR   = Errno(0x4)
	EIO     = Errno(0x5)
	EBADF   = Errno(0x9)
	EACCES  = Errno(0xd)
	EEXIST  = Errno(0x11)
	ENOTDIR = Errno(0x14)
	EISDIR  = Errno(0x15)
	EINVAL  = Errno(0x16)
	ENFILE  = Errno(0x17)
	EMFILE  = Errno(0x18)
	ENOSPC  = Errno(0x1c)
	EROFS   = Errno(0x1e)
	EPIPE   = Errno(0x20)
)

// The errno values whose number differs between Linux and Darwin: each is the
// running platform's.
var (
	EAGAIN      = Errno(sysConst("EAGAIN"))
	EWOULDBLOCK = Errno(sysConst("EWOULDBLOCK"))
	ETIMEDOUT   = Errno(sysConst("ETIMEDOUT"))
	ENOTEMPTY   = Errno(sysConst("ENOTEMPTY"))
	ENOSYS      = Errno(sysConst("ENOSYS"))
	ENOTSUP     = Errno(sysConst("ENOTSUP"))
	EOPNOTSUPP  = Errno(sysConst("EOPNOTSUPP"))
)

// The `open(2)` flags `os.OpenFile` takes, each the running platform's
// (`O_CREAT` is 0x40 on Linux and 0x200 on Darwin).
var (
	O_RDONLY = sysConst("O_RDONLY")
	O_WRONLY = sysConst("O_WRONLY")
	O_RDWR   = sysConst("O_RDWR")
	O_APPEND = sysConst("O_APPEND")
	O_CREAT  = sysConst("O_CREAT")
	O_EXCL   = sysConst("O_EXCL")
	O_SYNC   = sysConst("O_SYNC")
	O_TRUNC  = sysConst("O_TRUNC")
)
