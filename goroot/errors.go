// Copyright 2011 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style
// license that can be found in the LICENSE file.

// Package errors implements functions to manipulate errors.
//
// The [New] function creates errors whose only content is a text message.
//
// An error e wraps another error if e's type has one of the methods
//
//	Unwrap() error
//	Unwrap() []error
//
// If e.Unwrap() returns a non-nil error w or a slice containing w,
// then we say that e wraps w. A nil error returned from e.Unwrap()
// indicates that e does not wrap any error. It is invalid for an
// Unwrap method to return an []error containing a nil error value.
//
// An easy way to create wrapped errors is to call [fmt.Errorf] and apply
// the %w verb to the error argument:
//
//	wrapsErr := fmt.Errorf("... %w ...", ..., err, ...)
//
// Successive unwrapping of an error creates a tree. The [Is] and [As]
// functions inspect an error's tree by examining first the error
// itself followed by the tree of each of its children in turn
// (pre-order, depth-first traversal).
//
// See https://go.dev/blog/go1.13-errors for a deeper discussion of the
// philosophy of wrapping and when to wrap.
//
// [Is] examines the tree of its first argument looking for an error that
// matches the second. It reports whether it finds a match. It should be
// used in preference to simple equality checks:
//
//	if errors.Is(err, fs.ErrExist)
//
// is preferable to
//
//	if err == fs.ErrExist
//
// because the former will succeed if err wraps [io/fs.ErrExist].
//
// [AsType] examines the tree of its argument looking for an error whose
// type matches its type argument. If it succeeds, it returns the
// corresponding value of that type and true. Otherwise, it returns the
// zero value of that type and false. The form
//
//	if perr, ok := errors.AsType[*fs.PathError](err); ok {
//		fmt.Println(perr.Path)
//	}
//
// is preferable to
//
//	if perr, ok := err.(*fs.PathError); ok {
//		fmt.Println(perr.Path)
//	}
//
// because the former will succeed if err wraps an [*io/fs.PathError].
package errors

// New returns an error that formats as the given text.
// Each call to New returns a distinct error value even if the text is identical.
func New(text string) error {
	return &errorString{text}
}

// errorString is a trivial implementation of error.
type errorString struct {
	s string
}

func (e *errorString) Error() string {
	return e.s
}

// ErrUnsupported indicates that a requested operation cannot be performed,
// because it is unsupported. For example, a call to [os.Link] when using a
// file system that does not support hard links.
//
// Functions and methods should not return this error but should instead
// return an error including appropriate context that satisfies
//
//	errors.Is(err, errors.ErrUnsupported)
//
// either by directly wrapping ErrUnsupported or by implementing an [Is] method.
//
// Functions and methods should document the cases in which an error
// wrapping this will be returned.
var ErrUnsupported = New("unsupported operation")

// ── wrap.go ────────────────────────────────────────────────────────────────
//
// Ported from Go's `errors/wrap.go`. Go's `Is` consults
// `reflectlite.TypeOf(target).Comparable()` before comparing `err == target`;
// go-rs has no reflectlite, and every error value it can build (a pointer to a
// struct) is comparable, so that guard is the constant `true` here. `As` is
// lowered by the compiler onto [asTag] below, because Go's `As` writes through
// a `*T` target using reflectlite — go-rs resolves the target's type
// statically instead, which needs no reflection.

// Unwrap returns the result of calling the Unwrap method on err, if err's type
// contains an Unwrap method returning error. Otherwise, Unwrap returns nil.
//
// Unwrap only calls a method of the form "Unwrap() error". In particular Unwrap
// does not unwrap errors returned by [Join].
func Unwrap(err error) error {
	u, ok := err.(interface {
		Unwrap() error
	})
	if !ok {
		return nil
	}
	return u.Unwrap()
}

// Is reports whether any error in err's tree matches target.
//
// The tree consists of err itself, followed by the errors obtained by repeatedly
// calling its Unwrap() error or Unwrap() []error method. When err wraps multiple
// errors, Is examines err followed by a depth-first traversal of its children.
//
// An error is considered to match a target if it is equal to that target or if
// it implements a method Is(error) bool such that Is(target) returns true.
func Is(err, target error) bool {
	if err == nil || target == nil {
		return err == target
	}
	return is(err, target)
}

func is(err, target error) bool {
	for {
		if err == target {
			return true
		}
		if x, ok := err.(interface{ Is(error) bool }); ok && x.Is(target) {
			return true
		}
		switch x := err.(type) {
		case interface{ Unwrap() error }:
			err = x.Unwrap()
			if err == nil {
				return false
			}
		case interface{ Unwrap() []error }:
			for _, err := range x.Unwrap() {
				if is(err, target) {
					return true
				}
			}
			return false
		default:
			return false
		}
	}
}

// asTag walks err's tree for the first error whose dynamic type is `tag`,
// returning it and whether one was found. It is the target of the compiler's
// `errors.As(err, &t)` lowering: Go recovers the target's type from the pointer
// at run time with reflectlite, where go-rs already knows it at compile time and
// passes it here as its type tag. `runtimeTypeTag` is a host intrinsic — the
// same runtime type tag a type switch dispatches on. The traversal (err itself,
// then Unwrap() error, then a depth-first walk of Unwrap() []error) is Go's
// `as`; the one part of `as` not reproduced is the `As(any) bool` hook, which
// takes the pointer target this lowering replaces with a static type.
func asTag(err error, tag string) (any, bool) {
	for {
		if err == nil {
			return nil, false
		}
		if runtimeTypeTag(err) == tag {
			return err, true
		}
		switch x := err.(type) {
		case interface{ Unwrap() error }:
			err = x.Unwrap()
		case interface{ Unwrap() []error }:
			for _, e := range x.Unwrap() {
				if v, ok := asTag(e, tag); ok {
					return v, true
				}
			}
			return nil, false
		default:
			return nil, false
		}
	}
}

// ── join.go ────────────────────────────────────────────────────────────────

// Join returns an error that wraps the given errors. Any nil error values are
// discarded. Join returns nil if every value in errs is nil. The error formats
// as the concatenation of the strings obtained by calling the Error method of
// each element of errs, with a newline between each string.
//
// A non-nil error returned by Join implements the Unwrap() []error method.
func Join(errs ...error) error {
	n := 0
	for _, err := range errs {
		if err != nil {
			n++
		}
	}
	if n == 0 {
		return nil
	}
	e := &joinError{
		errs: make([]error, 0, n),
	}
	for _, err := range errs {
		if err != nil {
			e.errs = append(e.errs, err)
		}
	}
	return e
}

type joinError struct {
	errs []error
}

func (e *joinError) Error() string {
	// Since Join returns nil if every value in errs is nil, e.errs cannot be
	// empty.
	if len(e.errs) == 1 {
		return e.errs[0].Error()
	}
	// Go builds this in a []byte and hands the buffer to unsafe.String; the
	// observable result is the same concatenation.
	s := e.errs[0].Error()
	for _, err := range e.errs[1:] {
		s += "\n" + err.Error()
	}
	return s
}

func (e *joinError) Unwrap() []error {
	return e.errs
}
