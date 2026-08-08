// errors.Is / errors.As / errors.Unwrap / errors.Join and fmt.Errorf's %w:
// wrapping a cause, walking the chain, matching by identity (not by message),
// and the multi-error tree that Unwrap() []error produces.
package main

import (
	"errors"
	"fmt"
)

var ErrNotFound = errors.New("not found")

type queryError struct {
	query string
	code  int
}

func (e *queryError) Error() string {
	return fmt.Sprintf("query %q failed with %d", e.query, e.code)
}

// A sentinel-matching error: Is reports a match against ErrNotFound without
// wrapping it, which is the `Is(error) bool` hook errors.Is consults.
type missingError struct{ what string }

func (e *missingError) Error() string { return e.what + " is missing" }
func (e *missingError) Is(target error) bool {
	return target == ErrNotFound
}

func main() {
	// Two errors with the same text are distinct values.
	fmt.Println(errors.New("dup") == errors.New("dup"), ErrNotFound == ErrNotFound)

	// %w records the cause; %v does not.
	wrapped := fmt.Errorf("load config: %w", ErrNotFound)
	plain := fmt.Errorf("load config: %v", ErrNotFound)
	fmt.Println(wrapped)
	fmt.Println(errors.Is(wrapped, ErrNotFound), errors.Is(plain, ErrNotFound))
	fmt.Println(errors.Unwrap(wrapped) == ErrNotFound, errors.Unwrap(plain))
	fmt.Println(errors.Unwrap(ErrNotFound))

	// Chains nest: Is walks every hop.
	deep := fmt.Errorf("startup: %w", fmt.Errorf("stage 2: %w", wrapped))
	fmt.Println(deep)
	fmt.Println(errors.Is(deep, ErrNotFound), errors.Is(deep, errors.New("not found")))

	// As finds the first error of the target's type and assigns it.
	qe := &queryError{query: "SELECT 1", code: 42}
	ctx := fmt.Errorf("handler: %w", qe)
	var found *queryError
	if errors.As(ctx, &found) {
		fmt.Println("as:", found.code, found.query)
	}
	// A miss leaves the target alone and reports false.
	var absent *queryError
	fmt.Println(errors.As(ErrNotFound, &absent), absent == nil)

	// The Is(error) bool hook.
	fmt.Println(errors.Is(&missingError{"key"}, ErrNotFound))
	fmt.Println(errors.Is(fmt.Errorf("outer: %w", &missingError{"key"}), ErrNotFound))

	// Join builds a tree; Is and As walk it depth-first.
	joined := errors.Join(ErrNotFound, qe)
	fmt.Println(joined)
	fmt.Println(errors.Is(joined, ErrNotFound), errors.Is(joined, qe))
	var viaJoin *queryError
	fmt.Println(errors.As(joined, &viaJoin), viaJoin.code)
	fmt.Println(errors.Join() == nil, errors.Join(nil, nil) == nil)
	fmt.Println(errors.Join(ErrNotFound))

	// Several %w verbs wrap several errors at once.
	both := fmt.Errorf("first %w then %w", ErrNotFound, qe)
	fmt.Println(both)
	fmt.Println(errors.Is(both, ErrNotFound), errors.Is(both, qe))
	fmt.Println(errors.Unwrap(both))

	// A nil target and a nil error.
	var nilErr error
	fmt.Println(errors.Is(nilErr, nil), errors.Is(wrapped, nil), errors.Is(nilErr, ErrNotFound))
	fmt.Println(errors.Unwrap(nilErr))
}
