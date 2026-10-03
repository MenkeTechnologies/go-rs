// Copyright 2023 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style
// license that can be found in the LICENSE file.

// Package iter provides basic definitions and operations related to
// iterators over sequences.
//
// go-rs vendors the package without its runtime half. Go's own iter.go builds
// Pull and Pull2 on runtime coroutines (newcoro / coroswitch, linknamed into
// the runtime) and imports internal/race, runtime and unsafe for them; none of
// that loads from source. Seq and Seq2 are Go's declarations verbatim. Pull and
// Pull2 keep Go's contract — next runs seq up to its next yield, stop ends it,
// both are safe to call again after the sequence is over — but hand control
// between the caller and seq over unbuffered channels to a goroutine on the
// fusevm scheduler, which is the coroutine switch Go's runtime performs.
package iter

// Seq is an iterator over sequences of individual values.
// When called as seq(yield), seq calls yield(v) for each value v in the sequence,
// stopping early if yield returns false.
type Seq[V any] func(yield func(V) bool)

// Seq2 is an iterator over sequences of pairs of values, most commonly key-value pairs.
// When called as seq(yield), seq calls yield(k, v) for each pair (k, v) in the sequence,
// stopping early if yield returns false.
type Seq2[K, V any] func(yield func(K, V) bool)

// Pull converts the "push-style" iterator sequence seq
// into a "pull-style" iterator accessed by the two functions
// next and stop.
//
// Next returns the next value in the sequence
// and a boolean indicating whether the value is valid.
// When the sequence is over, next returns the zero V and false.
// It is valid to call next after reaching the end of the sequence
// or after calling stop. These calls will continue
// to return the zero V and false.
//
// Stop ends the iteration. It is valid to call stop multiple times
// and when next has already returned false.
func Pull[V any](seq Seq[V]) (func() (V, bool), func()) {
	var zero V
	started := false
	done := false
	resume := make(chan bool) // true: run to the next yield; false: stop
	out := make(chan V)       // closed once seq has returned
	run := func() {
		if <-resume {
			seq(func(v V) bool {
				out <- v
				return <-resume
			})
		}
		done = true
		close(out)
	}
	next := func() (V, bool) {
		if done {
			return zero, false
		}
		if !started {
			started = true
			go run()
		}
		resume <- true
		v, ok := <-out
		if !ok {
			return zero, false
		}
		return v, true
	}
	stop := func() {
		if done {
			return
		}
		if !started {
			done = true
			return
		}
		resume <- false
		<-out
	}
	return next, stop
}

// Pull2 converts the "push-style" iterator sequence seq
// into a "pull-style" iterator accessed by the two functions
// next and stop.
//
// Next returns the next pair in the sequence
// and a boolean indicating whether the pair is valid.
// When the sequence is over, next returns a pair of zero values and false.
// It is valid to call next after reaching the end of the sequence
// or after calling stop. These calls will continue
// to return a pair of zero values and false.
//
// Stop ends the iteration. It is valid to call stop multiple times
// and when next has already returned false.
func Pull2[K, V any](seq Seq2[K, V]) (func() (K, V, bool), func()) {
	var zk K
	var zv V
	started := false
	done := false
	resume := make(chan bool)
	outK := make(chan K)
	outV := make(chan V)
	run := func() {
		if <-resume {
			seq(func(k K, v V) bool {
				outK <- k
				outV <- v
				return <-resume
			})
		}
		done = true
		close(outK)
	}
	next := func() (K, V, bool) {
		if done {
			return zk, zv, false
		}
		if !started {
			started = true
			go run()
		}
		resume <- true
		k, ok := <-outK
		if !ok {
			return zk, zv, false
		}
		v := <-outV
		return k, v, true
	}
	stop := func() {
		if done {
			return
		}
		if !started {
			done = true
			return
		}
		resume <- false
		<-outK
	}
	return next, stop
}
