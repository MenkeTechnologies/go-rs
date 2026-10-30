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
	if wg.n < 0 {
		panic("sync: negative WaitGroup counter")
	}
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

// Go calls f in a new goroutine and adds that task to the WaitGroup.
// When f returns, the task is removed from the WaitGroup.
//
// (Go's own body.) If f panics, Done is not called: the panic is fatal in the
// new goroutine, and Wait must not race it to a clean exit.
func (wg *WaitGroup) Go(f func()) {
	wg.Add(1)
	go func() {
		defer func() {
			if x := recover(); x != nil {
				panic(x)
			}
			wg.Done()
		}()
		f()
	}()
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

// OnceFunc returns a function that invokes f only once. The returned function
// may be called concurrently.
//
// If f panics, the returned function will panic with the same value on every call.
//
// (Go's own body, `sync/oncefunc.go`.)
func OnceFunc(f func()) func() {
	d := struct {
		f     func()
		once  Once
		valid bool
		p     any
	}{
		f: f,
	}
	return func() {
		d.once.Do(func() {
			defer func() {
				d.f = nil
				d.p = recover()
				if !d.valid {
					panic(d.p)
				}
			}()
			d.f()
			d.valid = true
		})
		if !d.valid {
			panic(d.p)
		}
	}
}

// OnceValue returns a function that invokes f only once and returns the value
// returned by f. The returned function may be called concurrently.
//
// If f panics, the returned function will panic with the same value on every call.
func OnceValue[T any](f func() T) func() T {
	d := struct {
		f      func() T
		once   Once
		valid  bool
		p      any
		result T
	}{
		f: f,
	}
	return func() T {
		d.once.Do(func() {
			defer func() {
				d.f = nil
				d.p = recover()
				if !d.valid {
					panic(d.p)
				}
			}()
			d.result = d.f()
			d.valid = true
		})
		if !d.valid {
			panic(d.p)
		}
		return d.result
	}
}

// OnceValues returns a function that invokes f only once and returns the values
// returned by f. The returned function may be called concurrently.
//
// If f panics, the returned function will panic with the same value on every call.
func OnceValues[T1, T2 any](f func() (T1, T2)) func() (T1, T2) {
	d := struct {
		f     func() (T1, T2)
		once  Once
		valid bool
		p     any
		r1    T1
		r2    T2
	}{
		f: f,
	}
	return func() (T1, T2) {
		d.once.Do(func() {
			defer func() {
				d.f = nil
				d.p = recover()
				if !d.valid {
					panic(d.p)
				}
			}()
			d.r1, d.r2 = d.f()
			d.valid = true
		})
		if !d.valid {
			panic(d.p)
		}
		return d.r1, d.r2
	}
}

// Map is like a Go map[any]any but is safe for concurrent use by multiple
// goroutines. The zero Map is empty and ready for use.
//
// Go's is a concurrent hash-trie; with goroutines that never run at the same
// time a plain map is the whole of the contract. Like Go's, Range visits the
// entries in no particular order.
type Map struct {
	m map[any]any
}

// Load returns the value stored in the map for a key, or nil if no value is
// present. The ok result indicates whether value was found in the map.
func (m *Map) Load(key any) (value any, ok bool) {
	value, ok = m.m[key]
	return value, ok
}

// Store sets the value for a key.
func (m *Map) Store(key, value any) {
	if m.m == nil {
		m.m = map[any]any{}
	}
	m.m[key] = value
}

// Clear deletes all the entries, resulting in an empty Map.
func (m *Map) Clear() {
	m.m = nil
}

// LoadOrStore returns the existing value for the key if present. Otherwise, it
// stores and returns the given value. The loaded result is true if the value
// was loaded, false if stored.
func (m *Map) LoadOrStore(key, value any) (actual any, loaded bool) {
	if v, ok := m.m[key]; ok {
		return v, true
	}
	m.Store(key, value)
	return value, false
}

// LoadAndDelete deletes the value for a key, returning the previous value if
// any. The loaded result reports whether the key was present.
func (m *Map) LoadAndDelete(key any) (value any, loaded bool) {
	value, loaded = m.m[key]
	delete(m.m, key)
	return value, loaded
}

// Delete deletes the value for a key.
func (m *Map) Delete(key any) {
	delete(m.m, key)
}

// Swap swaps the value for a key and returns the previous value if any. The
// loaded result reports whether the key was present.
func (m *Map) Swap(key, value any) (previous any, loaded bool) {
	previous, loaded = m.m[key]
	m.Store(key, value)
	return previous, loaded
}

// CompareAndSwap swaps the old and new values for key if the value stored in
// the map is equal to old. The old value must be of a comparable type.
func (m *Map) CompareAndSwap(key, old, new any) (swapped bool) {
	if v, ok := m.m[key]; ok && v == old {
		m.m[key] = new
		return true
	}
	return false
}

// CompareAndDelete deletes the entry for key if its value is equal to old. The
// old value must be of a comparable type.
func (m *Map) CompareAndDelete(key, old any) (deleted bool) {
	if v, ok := m.m[key]; ok && v == old {
		delete(m.m, key)
		return true
	}
	return false
}

// Range calls f sequentially for each key and value present in the map. If f
// returns false, range stops the iteration.
func (m *Map) Range(f func(key, value any) bool) {
	for k, v := range m.m {
		if !f(k, v) {
			break
		}
	}
}

// A Pool is a set of temporary objects that may be individually saved and
// retrieved. New optionally specifies a function to generate a value when Get
// would otherwise return nil.
//
// Go's pool may drop any item at any time; this one keeps every item Put until
// a Get takes it, most recently Put first, which is what Go's per-P private
// slot returns to a single goroutine.
type Pool struct {
	items []any
	New   func() any
}

// Put adds x to the pool.
func (p *Pool) Put(x any) {
	if x == nil {
		return
	}
	p.items = append(p.items, x)
}

// Get selects an arbitrary item from the Pool, removes it from the Pool, and
// returns it to the caller. If Get would otherwise return nil and p.New is
// non-nil, Get returns the result of calling p.New.
func (p *Pool) Get() any {
	if n := len(p.items); n > 0 {
		x := p.items[n-1]
		p.items = p.items[:n-1]
		return x
	}
	if p.New != nil {
		return p.New()
	}
	return nil
}
