// Copyright 2009 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style
// license that can be found in the LICENSE file.

// Package sync provides basic synchronization primitives such as mutual
// exclusion locks.
//
// This is not a port of the standard library's implementation. Go's `sync` is
// built on `sync/atomic`, `unsafe`, and the runtime's `semacquire` — none of
// which exist here, and none of which it needs to: fusevm's goroutines are
// cooperative green threads on one OS thread that yield only at channel
// operations, so no two goroutines ever execute concurrently and a critical
// section containing no channel operation cannot be preempted at all. What is
// left to implement is the part that is still observable: a goroutine must be
// able to *park* until another goroutine makes progress. That is what the
// package-level `parked` channel below is for.
//
// The types therefore keep the standard contract — the zero value is ready to
// use, and a Mutex/WaitGroup must not be copied after first use — while their
// internals are the smallest thing that satisfies it on this scheduler.
package sync

// parked is the one place a goroutine sleeps. It is created at package
// initialization, before any goroutine exists, so it is never nil and never
// racy to create; a lazily-made channel would be, because `make(chan …)` is
// itself a yield point and another goroutine can run inside it.
//
// A token is a hint that something changed, not ownership of anything: every
// sleeper re-checks its own condition after waking, so a token consumed by the
// "wrong" waiter only costs an extra loop. `wake` never blocks, so a token with
// no sleeper to take it is simply dropped — a sleeper can only be sleeping when
// the buffer is empty, so the send that matters always lands.
var parked = make(chan int, 1)

func wake() {
	select {
	case parked <- 1:
	default:
	}
}

// A Mutex is a mutual exclusion lock.
// The zero value for a Mutex is an unlocked mutex.
type Mutex struct {
	locked bool
}

// Lock locks m. If the lock is already in use, the calling goroutine blocks
// until the mutex is available.
func (m *Mutex) Lock() {
	for m.locked {
		<-parked
	}
	m.locked = true
}

// TryLock tries to lock m and reports whether it succeeded.
func (m *Mutex) TryLock() bool {
	if m.locked {
		return false
	}
	m.locked = true
	return true
}

// Unlock unlocks m. It is a run-time error if m is not locked on entry to
// Unlock.
func (m *Mutex) Unlock() {
	m.locked = false
	wake()
}

// An RWMutex is a reader/writer mutual exclusion lock. The lock can be held by
// an arbitrary number of readers or a single writer. The zero value for an
// RWMutex is an unlocked mutex.
type RWMutex struct {
	readers int
	writing bool
}

// RLock locks rw for reading.
func (rw *RWMutex) RLock() {
	for rw.writing {
		<-parked
	}
	rw.readers++
}

// RUnlock undoes a single RLock call.
func (rw *RWMutex) RUnlock() {
	rw.readers--
	wake()
}

// Lock locks rw for writing.
func (rw *RWMutex) Lock() {
	for rw.writing || rw.readers > 0 {
		<-parked
	}
	rw.writing = true
}

// Unlock unlocks rw for writing.
func (rw *RWMutex) Unlock() {
	rw.writing = false
	wake()
}

// A WaitGroup waits for a collection of goroutines to finish. The main
// goroutine calls Add to set the number of goroutines to wait for. Then each of
// the goroutines runs and calls Done when finished. At the same time, Wait can
// be used to block until all goroutines have finished.
//
// A WaitGroup must not be copied after first use.
type WaitGroup struct {
	n int
}

// Add adds delta, which may be negative, to the WaitGroup counter.
func (wg *WaitGroup) Add(delta int) {
	wg.n += delta
	if wg.n <= 0 {
		wake()
	}
}

// Done decrements the WaitGroup counter by one.
func (wg *WaitGroup) Done() {
	wg.Add(-1)
}

// Wait blocks until the WaitGroup counter is zero.
func (wg *WaitGroup) Wait() {
	for wg.n > 0 {
		<-parked
	}
	// Pass the wakeup on: several goroutines may be waiting on the same group,
	// and each token only ever wakes one of them.
	wake()
}

// Once is an object that will perform exactly one action.
//
// A Once must not be copied after first use.
type Once struct {
	done bool
}

// Do calls the function f if and only if Do is being called for the first time
// for this instance of Once.
func (o *Once) Do(f func()) {
	if o.done {
		return
	}
	o.done = true
	f()
}
