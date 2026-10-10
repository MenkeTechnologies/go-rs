// Package atomic provides the low-level atomic memory primitives of Go's
// `sync/atomic`.
//
// Not a port of the standard library's implementation, which is assembly and
// compiler intrinsics. go-rs's goroutines are cooperative green threads on one
// OS thread that yield only at channel operations, so a read-modify-write
// sequence containing no channel operation cannot be interleaved: each
// function below is atomic by construction.
package atomic

func AddInt32(addr *int32, delta int32) (new int32) {
	*addr += delta
	return *addr
}

func LoadInt32(addr *int32) (val int32) { return *addr }

func StoreInt32(addr *int32, val int32) { *addr = val }

func SwapInt32(addr *int32, new int32) (old int32) {
	old = *addr
	*addr = new
	return old
}

func CompareAndSwapInt32(addr *int32, old, new int32) (swapped bool) {
	if *addr == old {
		*addr = new
		return true
	}
	return false
}

// A Int32 is an atomic int32. The zero value is zero.
type Int32 struct {
	v int32
}

func (x *Int32) Load() int32 { return x.v }

func (x *Int32) Store(val int32) { x.v = val }

func (x *Int32) Swap(new int32) (old int32) {
	old = x.v
	x.v = new
	return old
}

func (x *Int32) CompareAndSwap(old, new int32) (swapped bool) {
	if x.v == old {
		x.v = new
		return true
	}
	return false
}

func (x *Int32) Add(delta int32) (new int32) {
	x.v += delta
	return x.v
}

func AddInt64(addr *int64, delta int64) (new int64) {
	*addr += delta
	return *addr
}

func LoadInt64(addr *int64) (val int64) { return *addr }

func StoreInt64(addr *int64, val int64) { *addr = val }

func SwapInt64(addr *int64, new int64) (old int64) {
	old = *addr
	*addr = new
	return old
}

func CompareAndSwapInt64(addr *int64, old, new int64) (swapped bool) {
	if *addr == old {
		*addr = new
		return true
	}
	return false
}

// A Int64 is an atomic int64. The zero value is zero.
type Int64 struct {
	v int64
}

func (x *Int64) Load() int64 { return x.v }

func (x *Int64) Store(val int64) { x.v = val }

func (x *Int64) Swap(new int64) (old int64) {
	old = x.v
	x.v = new
	return old
}

func (x *Int64) CompareAndSwap(old, new int64) (swapped bool) {
	if x.v == old {
		x.v = new
		return true
	}
	return false
}

func (x *Int64) Add(delta int64) (new int64) {
	x.v += delta
	return x.v
}

func AddUint32(addr *uint32, delta uint32) (new uint32) {
	*addr += delta
	return *addr
}

func LoadUint32(addr *uint32) (val uint32) { return *addr }

func StoreUint32(addr *uint32, val uint32) { *addr = val }

func SwapUint32(addr *uint32, new uint32) (old uint32) {
	old = *addr
	*addr = new
	return old
}

func CompareAndSwapUint32(addr *uint32, old, new uint32) (swapped bool) {
	if *addr == old {
		*addr = new
		return true
	}
	return false
}

// A Uint32 is an atomic uint32. The zero value is zero.
type Uint32 struct {
	v uint32
}

func (x *Uint32) Load() uint32 { return x.v }

func (x *Uint32) Store(val uint32) { x.v = val }

func (x *Uint32) Swap(new uint32) (old uint32) {
	old = x.v
	x.v = new
	return old
}

func (x *Uint32) CompareAndSwap(old, new uint32) (swapped bool) {
	if x.v == old {
		x.v = new
		return true
	}
	return false
}

func (x *Uint32) Add(delta uint32) (new uint32) {
	x.v += delta
	return x.v
}

func AddUint64(addr *uint64, delta uint64) (new uint64) {
	*addr += delta
	return *addr
}

func LoadUint64(addr *uint64) (val uint64) { return *addr }

func StoreUint64(addr *uint64, val uint64) { *addr = val }

func SwapUint64(addr *uint64, new uint64) (old uint64) {
	old = *addr
	*addr = new
	return old
}

func CompareAndSwapUint64(addr *uint64, old, new uint64) (swapped bool) {
	if *addr == old {
		*addr = new
		return true
	}
	return false
}

// A Uint64 is an atomic uint64. The zero value is zero.
type Uint64 struct {
	v uint64
}

func (x *Uint64) Load() uint64 { return x.v }

func (x *Uint64) Store(val uint64) { x.v = val }

func (x *Uint64) Swap(new uint64) (old uint64) {
	old = x.v
	x.v = new
	return old
}

func (x *Uint64) CompareAndSwap(old, new uint64) (swapped bool) {
	if x.v == old {
		x.v = new
		return true
	}
	return false
}

func (x *Uint64) Add(delta uint64) (new uint64) {
	x.v += delta
	return x.v
}

func AddUintptr(addr *uintptr, delta uintptr) (new uintptr) {
	*addr += delta
	return *addr
}

func LoadUintptr(addr *uintptr) (val uintptr) { return *addr }

func StoreUintptr(addr *uintptr, val uintptr) { *addr = val }

func SwapUintptr(addr *uintptr, new uintptr) (old uintptr) {
	old = *addr
	*addr = new
	return old
}

func CompareAndSwapUintptr(addr *uintptr, old, new uintptr) (swapped bool) {
	if *addr == old {
		*addr = new
		return true
	}
	return false
}

// A Uintptr is an atomic uintptr. The zero value is zero.
type Uintptr struct {
	v uintptr
}

func (x *Uintptr) Load() uintptr { return x.v }

func (x *Uintptr) Store(val uintptr) { x.v = val }

func (x *Uintptr) Swap(new uintptr) (old uintptr) {
	old = x.v
	x.v = new
	return old
}

func (x *Uintptr) CompareAndSwap(old, new uintptr) (swapped bool) {
	if x.v == old {
		x.v = new
		return true
	}
	return false
}

func (x *Uintptr) Add(delta uintptr) (new uintptr) {
	x.v += delta
	return x.v
}

// A Bool is an atomic boolean value. The zero value is false.
type Bool struct {
	v bool
}

func (x *Bool) Load() bool { return x.v }

func (x *Bool) Store(val bool) { x.v = val }

func (x *Bool) Swap(new bool) (old bool) {
	old = x.v
	x.v = new
	return old
}

func (x *Bool) CompareAndSwap(old, new bool) (swapped bool) {
	if x.v == old {
		x.v = new
		return true
	}
	return false
}

// A Value provides an atomic load and store of a consistently typed value.
type Value struct {
	v any
}

func (v *Value) Load() (val any) { return v.v }

func (v *Value) Store(val any) {
	if val == nil {
		panic("sync/atomic: store of nil value into Value")
	}
	v.v = val
}

func (v *Value) Swap(new any) (old any) {
	if new == nil {
		panic("sync/atomic: swap of nil value into Value")
	}
	old = v.v
	v.v = new
	return old
}

func (v *Value) CompareAndSwap(old, new any) (swapped bool) {
	if new == nil {
		panic("sync/atomic: compare and swap of nil value into Value")
	}
	if v.v == old {
		v.v = new
		return true
	}
	return false
}
