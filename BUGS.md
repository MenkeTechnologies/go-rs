# Known parity gaps

Behavioural differences between go-rs and the reference `go` toolchain that are
known and not yet closed. Each entry is a reproducer, what Go prints, what go-rs
prints, and what it would take to close.

Found by the differential harnesses:

- `bash parity-scripts/run.sh` — byte-diffs every `parity-scripts/**/*.go`
  against `go run`, and prints the rate. It is a green gate: every file matches.
- `cargo build --bin parity-fuzz && ./target/debug/parity-fuzz --count 20000`
  — generated deterministic-output programs, byte-diffed the same way.

A gap listed here is deliberately **not** represented by a corpus file, because
the corpus is a green byte-parity gate. Close the gap and add the corpus file in
the same change.

## `float32` prints with `float64` precision

```go
var f := float32(1.0 / 3.0)
fmt.Println(f)  // go: 0.33333334     go-rs: 0.3333333333333333
```

The conversion itself is right — `float32(x)` does round to `f32` — but `fmt`
formats every float with the shortest representation that round-trips as an
`f64`. Go picks the shortest that round-trips at the value's own width.

Unlike the integer case — now closed by `emit_narrow` in `src/compiler.rs` —
this one cannot be fixed by emitting ops around
the arithmetic: the width is not needed at the *operation*, it is needed at the
*print*, which happens in a host builtin that only ever sees a `Value::Float`.
Closing it needs the value itself to carry its width — a `Value` variant or a
compile-time-known formatting hint threaded into `fmt` — not a lowering trick.

## A nil slice or nil map prints as `<nil>`

```go
var s []int
var m map[string]int
fmt.Println(s, m)   // go: [] map[]     go-rs: <nil> <nil>
fmt.Println(m["x"]) // go: 0            go-rs: go-rs: invalid index of nil
```

`len`, `cap` and `append` on a nil slice are already correct. The zero value is
`Value::Undef`, which carries no element type, so the printer cannot tell a nil
slice from a nil map from a nil interface.

**Tractability of the equality hook (assessed).** The blocker recorded here
previously was that a distinguished nil object would print right but `s == nil`
would then be false, and that go-rs installs no equality hook to fix it. That is
no longer true: `numeric_hook` in `src/host.rs` already receives `NumOp::Eq` /
`NumOp::Ne` for every comparison whose operands are not both numeric, and the
pointer-identity rule added for `errors.Is` (`ptr_eq`) is exactly such a hook.
A `HostObj::NilSlice` / `HostObj::NilMap` comparing equal to `Value::Undef` is
the same three lines. So the hook is **not** the obstacle.

What remains is plumbing, and it is the larger half:

- `emit_default` (`src/compiler.rs`) is handed a `NumType`, which has collapsed
  `[]int`, `map[K]V` and `any` into one `Unknown`. It needs the written type to
  choose which distinguished nil to emit, so its callers must pass it.
- Every builtin that consumes a slice or map (`GLEN`, `GCAP`, `GAPPEND`,
  `GINDEX_GET`, `GRANGE_KEYS`, `GCOPY`, `GMAP_GET2`, `GDELETE`) has to treat the
  new objects as empty, and `GINDEX_SET` on a nil map has to panic with Go's
  "assignment to entry in nil map" rather than go-rs's current fault text.

Both are mechanical; neither is a substrate gap. This is a plumbing task, not a
blocked one.

## A pointer to a struct prints without `&`

```go
p := point{1, "a"}
fmt.Println(&p)             // go: &{1 a}          go-rs: {1 a}
fmt.Println(outer{p: &p})   // go: {0xc000010030}  go-rs: {{1 a}}
```

A *heap-allocated* pointer is now distinguishable at run time — `&T{…}` and
`new(T)` mark their handle `by_ref` (`HostObj::Struct`, `src/host.rs`), which is
what makes `==` compare them by identity. Two things still block the printing
half:

- `&x` on an existing variable is a no-op on the shared handle, so it cannot be
  marked without also marking `x`, which would wrongly make `x == y` compare by
  identity for the plain struct value. It needs a real pointer wrapper object
  (with the deref plumbing that implies), not a flag.
- A pointer *nested* inside a printed value is a hex address in Go, which is
  nondeterministic and not reproducible at all; the depth-0 `&{…}` form is the
  only part worth matching.

## `append` capacity misses Go's malloc size-class rounding

```go
var s []int
s = append(s, 1, 2, 3, 4, 5)
fmt.Println(cap(s))   // go: 6     go-rs: 5
```

`runtime.nextslicecap` is ported faithfully (see `next_slice_cap` in
`src/host.rs`), so the repeated-single-append doubling sequence
`1 2 4 4 8 8 8 8 16 …` matches exactly. Go then rounds the new backing array's
**byte** size up to a malloc size class — 5 ints is 40 bytes, which rounds to
the 48-byte class, giving cap 6. go-rs has no static element type at run time,
so it cannot compute the byte size. Sniffing it from the element values would
give the wrong answer for `[]byte` (1 byte) and for struct elements, so it is
left unrounded rather than confidently wrong.

## `for v := range ch` yields nothing

```go
ch := make(chan int, 3)
ch <- 1; ch <- 2; ch <- 3
close(ch)
for v := range ch { fmt.Println(v) }   // go: 1 2 3     go-rs: (no iterations)
```

Ranging a channel silently produces no values (the channel is a `Value::Int`
handle, so the range lowers onto Go 1.22's range-over-int and iterates the
handle's *id*). Every other channel operation — send, receive, `close`,
`select`, buffered and unbuffered — is correct; receive in a counted loop
meanwhile.

Closing it needs fusevm, not go-rs: `Scheduler::recv` returns the frontend's
`recv_zero` for a drained closed channel and reports no "closed" flag, so a
receive cannot tell a closed channel from one that delivered a real zero. A
`v, ok := <-ch` result (or a `chan_closed(ch)` query paired with a length query)
would make the loop expressible; setting `recv_zero` to a sentinel instead would
regress plain `<-ch` on a closed `chan int`, which correctly yields `0` today.

## A `strconv` error is not a `*strconv.NumError`

```go
_, err := strconv.Atoi("xx")
fmt.Println(err)                            // matches Go exactly
errors.Is(err, strconv.ErrSyntax)           // go: true    go-rs: undefined
var ne *strconv.NumError; errors.As(err, &ne)  // go: true  go-rs: undefined
```

The error *text* is Go's, and the value is a real error (non-nil, printable,
compares by identity). What is missing is its type: Go returns a
`*strconv.NumError` wrapping `strconv.ErrSyntax` / `strconv.ErrRange`, whereas
go-rs returns the same `&$errorString{s: …}` that `fmt.Errorf` builds. Closing
it needs `strconv` vendored as Go source (it is a native host package today,
because it reaches the float-formatting runtime), or a host-side `NumError`
struct plus the two sentinel errors exported as package constants.

## `var a, b int = 1, 2` does not parse

```go
var wide, wider int = 300, 5000000000
// go-rs: unexpected token `Comma` in expression
```

A `var` declaration binds one name. The multi-name forms — with or without an
initializer list — are unsupported; `a, b := 1, 2` and separate `var`
statements both work. `Stmt::Var` holds a single `name`, so closing it means
either widening that node or desugaring one declaration into several.

## A failed anonymous-interface assertion names its method set, not its signature

```go
var err error = errors.New("x")
_ = err.(interface{ Unwrap() error })
// go:    panic: interface conversion: *errors.errorString is not
//        interface { Unwrap() error }: missing method Unwrap
// go-rs: panic: interface conversion: main.$errorString is not
//        interface{Unwrap/0:error}: missing method Unwrap
```

Whether the assertion succeeds is right — that is method-set containment on
signatures, and it is what `errors.Is`/`As` rely on. Only the panic *text*
differs, because the parser canonicalizes an inline interface to
[`method_sig`]-encoded names (`src/ast.rs`) rather than keeping the written
source, which it cannot recover: tokens carry a line but no byte offset.

## A conversion to an interface type is rejected

```go
_ = error(myErr{})   // go-rs: undefined: error
_ = any(3)
```

`T(x)` is accepted for the builtin scalar types and `[]byte`/`[]rune`; naming an
interface type in call position is read as a call to an undefined function.
Assigning through a declared variable (`var e error = myErr{}`) is the working
spelling. Closing it means treating a known interface name in call position as
the identity conversion it is.

## Unsupported stdlib calls

`fmt.Fprintln` / `fmt.Fprintf` (writer-directed output) and
`strconv.FormatFloat` are rejected at compile time.

## Constant-overflow is not diagnosed

```go
fmt.Println(int8(300))
// go:    compile error — constant 300 overflows int8
// go-rs: 44
```

go-rs has no constant-range checking pass, so an out-of-range constant
conversion silently truncates instead of failing the build.
