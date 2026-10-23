// os.Open / ReadFile / WriteFile / Remove over paths whose outcome is the same
// on every machine — /dev/null, the root directory, a path that cannot exist —
// so the corpus needs no scratch files: the errors are *fs.PathError values
// wrapping syscall.Errno, matched by errors.Is / errors.As / os.IsNotExist.
package main

import (
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"syscall"
)

func main() {
	// A missing file: open fails with ENOENT.
	data, err := os.ReadFile("/nonexistent-go-rs/file.txt")
	fmt.Println(data == nil, len(data), err)
	fmt.Printf("%T\n", err)
	fmt.Println(errors.Is(err, fs.ErrNotExist), errors.Is(err, os.ErrNotExist), os.IsNotExist(err))
	fmt.Println(errors.Is(err, os.ErrExist), os.IsExist(err), os.IsPermission(err))
	var pe *os.PathError
	if errors.As(err, &pe) {
		fmt.Println("op:", pe.Op, "path:", pe.Path, "err:", pe.Err)
		fmt.Printf("%T %v\n", pe.Err, pe.Err == syscall.ENOENT)
	}
	if e, ok := errors.Unwrap(err).(syscall.Errno); ok {
		fmt.Println("errno", int(e), e.Timeout(), e.Temporary())
	}

	// The same error through Open, and the nil *File it returns.
	f, err := os.Open("/nonexistent-go-rs")
	fmt.Println(f == nil, err)

	// WriteFile into a directory that does not exist.
	err = os.WriteFile("/nonexistent-go-rs/out.txt", []byte("x"), 0644)
	fmt.Println(err, os.IsNotExist(err))

	// Remove reports the unlink error when rmdir says ENOTDIR or ENOENT.
	err = os.Remove("/nonexistent-go-rs")
	fmt.Println(err, os.IsNotExist(err))

	// /dev/null reads as empty and accepts every write.
	data, err = os.ReadFile("/dev/null")
	fmt.Println(len(data), cap(data), err)
	fmt.Println(os.WriteFile("/dev/null", []byte("discarded\n"), 0644))

	nul, err := os.OpenFile("/dev/null", os.O_WRONLY, 0)
	fmt.Println(err, nul.Name())
	n, err := nul.WriteString("hello")
	fmt.Println(n, err)
	n, err = fmt.Fprintf(nul, "%d-%s", 42, "x")
	fmt.Println(n, err)
	fmt.Println(nul.Close())
	// A second Close, and any I/O after the first, report ErrClosed.
	err = nul.Close()
	fmt.Println(err, errors.Is(err, os.ErrClosed))
	n, err = nul.Write([]byte("late"))
	fmt.Println(n, err)

	// Reading /dev/null through the io.Reader the *File is.
	r, _ := os.Open("/dev/null")
	all, err := io.ReadAll(r)
	fmt.Println(len(all), err)
	buf := make([]byte, 8)
	n, err = r.Read(buf)
	fmt.Println(n, err, err == io.EOF)
	r.Close()
	n, err = r.Read(buf)
	fmt.Println(n, err)

	// A directory opens read-only, but read(2) on it fails with EISDIR.
	dir, err := os.Open("/")
	fmt.Println(err)
	n, err = dir.Read(buf)
	fmt.Println(n, err)
	_, err = os.ReadFile("/")
	fmt.Println(err)
	dir.Close()

	// Errno text and the sentinels Errno.Is maps onto.
	fmt.Println(syscall.ENOENT, syscall.EACCES, syscall.EISDIR, syscall.EEXIST)
	fmt.Println(errors.Is(syscall.EACCES, fs.ErrPermission), errors.Is(syscall.EEXIST, fs.ErrExist))
	fmt.Println(errors.Is(syscall.ENOTEMPTY, fs.ErrExist), errors.Is(syscall.ENOSYS, errors.ErrUnsupported))
	fmt.Println(fs.ErrNotExist, os.ErrInvalid, os.ErrPermission, fs.ErrClosed)
	fmt.Println(os.ErrNotExist == fs.ErrNotExist)

	// FileMode's methods.
	var m os.FileMode = 0755
	fmt.Println(m.Perm(), m.IsDir(), m.IsRegular(), uint32(fs.ModeDir|m))
	fmt.Println(fs.ModeDir.IsDir(), uint32(fs.ModePerm))

	// The flag values are the platform's.
	fmt.Println(os.O_RDONLY == syscall.O_RDONLY, os.O_CREATE == syscall.O_CREAT, os.O_TRUNC == syscall.O_TRUNC)
	fmt.Println(os.Stdin.Name(), os.Stdout.Name(), os.Stderr.Name())
}
