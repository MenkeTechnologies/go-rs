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

## `for v := range ch` yields nothing — waiting on a fusevm release

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

The substrate gap is closed but not yet reachable. `Scheduler::recv` returned
the frontend's `recv_zero` for a drained closed channel with no "closed" flag,
so a receive could not tell a closed channel from one that delivered a real
zero. fusevm landed `Op::ChanRecvOk` (commit `ff299f4a8a`) for exactly this,
but `Cargo.toml` pins `fusevm = "0.17.0"` from crates.io, which predates it.
**This stays open until fusevm publishes a release carrying `Op::ChanRecvOk`**;
the fix is then a `v, ok := <-ch` lowering in `compile_for_range`, not new
design work. Vendoring or path-overriding fusevm to reach the op early is not
an option — the published pin is the contract.

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

## `%T` after `fmt`'s `Stringer` dispatch names the rendered type

```go
var v any = myErr{"e"}       // myErr has an Error() string method
fmt.Printf("%T\n", v)        // go: main.myErr    go-rs: string
```

Every `fmt` argument is wrapped in the linker-synthesized `$stringify`, which
calls `Error()`/`String()` on the types that have one — that is how a value
implementing `error` prints through its method. `%T` is the one verb that wants
the operand *before* that dispatch, and it sees the `string` the wrapper
returned. A value whose type has no such method is unaffected, as is `%T` on a
concrete variable.

Closing it means not wrapping the arguments a literal format string sends to
`%T`: the compiler already has the format string at the call site, so it can map
verb positions to argument positions and skip the wrapper for those. It needs a
format-string scan in `src/compiler.rs`, and a non-literal format string (a
variable) would still go through the wrapper.

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
conversion silently truncates instead of failing the build. The same pass would
catch `float32(1e20) * float32(1e20)` (constant overflow of `float32`) and
`x / 0` on constants.

## Constant folding keeps a signed zero

```go
fmt.Println(-float32(0))   // go: 0     go-rs: -0
```

Go's constants are exact rationals with no signed zero, so `-float32(0)` folds
to `0` at compile time. go-rs evaluates the negation at run time on an IEEE
`f64`, which does have `-0`. A *variable* zero is right in both (`z := float32(0);
-z` prints `-0` in each). Closing it needs the same constant-evaluation pass the
overflow diagnosis above wants.

## `float32` precision is not tracked across a function boundary or a nested struct

```go
func id(v any) any { return v }
var f float32 = 1.0 / 3.0
fmt.Println(id(f))                    // go: 0.33333334     go-rs: 0.3333333432674408
fmt.Println(outer{in: inner{f: f}})   // go: {{0.33333334}}  go-rs: {{0.3333333432674408}}
```

The *value* is right in both cases — the conversion rounded to `f32` — it is
only rendered at 64-bit shortest precision.

`float32` arithmetic and printing are otherwise correct: every operation runs at
32-bit width (`GF32_ARITH`), and a statically-`float32` `fmt` argument is tagged
with its width on the way in (`GF32_BOX`) so `fmt` renders the 32-bit shortest
decimal. The tag is applied from the *static* type at the call site, so the two
places the static type is gone are the two gaps: a value that has passed through
an `any`/interface parameter, and a `float32` field one struct deeper than the
argument's own type (the tag walks slices, maps and the argument struct's own
fields, but does not recurse into a struct-typed field).

Closing it needs the width to live on the value rather than at the call site —
a `Value` variant, which is the same change `%T`-through-`any` would want.
