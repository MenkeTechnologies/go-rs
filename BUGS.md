# Known parity gaps

Behavioural differences between go-rs and the reference `go` toolchain that are
known and not yet closed. Each entry is a reproducer, what Go prints, what go-rs
prints, and what it would take to close.

Found by the differential harnesses:

- `bash parity-scripts/run.sh` — byte-diffs every `parity-scripts/**/*.go`
  against `go run`, and prints the rate. It is a green gate: every file matches.
- `cargo build --bin parity-fuzz && ./target/debug/parity-fuzz --count 20000`
  — generated deterministic-output programs, byte-diffed the same way. A case
  counts only when the reference itself ran (exit 0, non-empty stdout); the
  rest are reported as `skipped`, so a generator slip that makes `go` reject
  the program cannot read as agreement. `--only N` pins the generated shape and
  `--ours PATH` runs a binary from another commit, which is how a new shape is
  shown to fail against the code from *before* the fix it claims to cover.

A gap listed here is deliberately **not** represented by a corpus file, because
the corpus is a green byte-parity gate. Close the gap and add the corpus file in
the same change.

## `len(ch)` / `cap(ch)` report 0 — waiting on a fusevm release

```go
ch := make(chan int, 1)
ch <- 1
fmt.Println(len(ch), cap(ch))   // go: 1 1     go-rs: 0 0
```

The scheduler owns the channel buffer, and fusevm 0.17.0's channel surface is
`Op::ChanMake` / `ChanSend` / `ChanRecv` / `ChanClose` / `Select` — there is no
op that reads a channel's length or capacity, so the frontend has nothing to
ask and `len`/`cap` fall through to their "not a container" answer of 0. Every
other channel operation is correct. Closing this needs a fusevm release
carrying a channel-length op; vendoring or path-overriding fusevm to add one is
not an option — the published pin is the contract.

## A defined type's name does not survive assignment to an interface

```go
type Weekday int
var a any = Weekday(3)
fmt.Printf("%T\n", a)   // go: main.Weekday   go-rs: int
```

A defined type is represented exactly like its base, so its name lives in the
static type; `named_box_spec` (`src/compiler.rs`) reads that at the `fmt` call
site and tags the operand. An operand whose static type is `any` has no name to
read — the same erasure that makes a `float32` or a `uint64` lose its width
through an `any` parameter. Closing it needs the name on the value rather than
at the call site, which is a representation change: a defined type would stop
being free.

A `*Weekday` is named `main.Weekday` rather than `*main.Weekday`, for the
reason in the pointer entry below: go-rs holds a pointer and its pointee as one
handle.

## `%T` of an empty map describes it from its contents

```go
fmt.Printf("%T\n", map[string]int{})   // go: map[string]int
                                       // go-rs: map[interface {}]interface {}
```

A map carries no element type on the object, so `go_type_name` (`src/host.rs`)
names it from the first pair — and an empty one has none. A map whose written
type mentions a *defined* type is tagged at the `fmt` call site and so is named
exactly (`map[main.myStr]main.myInt`, even when empty); a map of predeclared
types is not, because tagging every map would put a box on the common path for
a name that is almost always already right. A slice does not have this gap: its
element type is stamped by `GELEM_TAG`.

## An unassigned code point prints literally where Go escapes it

```go
fmt.Printf("%q\n", 0x378)   // go: '͸'   go-rs: '͸'
```

`strconv.Quote` writes a rune literally when `unicode.IsPrint` accepts it —
every letter, mark, number, punctuation and symbol, plus the ASCII space.
`go_is_print` (`src/host.rs`) decides three of the four non-printable classes
exactly: the C0 and C1 controls are `char::is_control`, every separator but the
ASCII space is `char::is_whitespace`, and the private-use areas are three fixed
ranges. The fourth is `Cn`, the code points Unicode has not assigned, which is
neither a fixed range nor derivable from anything Rust's standard library
exposes — it needs the general-category tables, and those change with the
Unicode version. Closing it means carrying a category table (or a generated
`IsPrint` range list) in the frontend.

## A call passes at most 255 arguments

```go
fmt.Println(0, 1, 2, /* … 256 arguments in total … */)
// go:    prints them all
// go-rs: compile error — `fmt.Println` takes at most 255 arguments here
```

fusevm 0.17.0's `Op::CallBuiltin` carries its argument count in a `u8`
(`CallBuiltin(u16, u8)`, and `BuiltinHandler = fn(&mut VM, u8)`), so one call
can take at most 255 stack values. A *composite literal* is not bounded by this
— it is built in chunks, the first through its literal builtin and each later
one through `GLIT_EXTEND`, so `[]int{…}` and `map[K]V{…}` and a struct literal
work at any size. A call site has no container to build up, so the count cannot
be chunked away and go-rs refuses to build instead.

This was a silent truncation until the count was checked: the byte wrapped, and
`fmt.Println` with 256 arguments printed a blank line. Raising the bound needs a
fusevm release with a wider arity encoding; the published pin is the contract.

Array and struct **value** semantics are implemented at every copy site —
assignment, argument bind, return, container store, container read, `range`
(over the array and for the element binding), channel send, `append` including
a spread, and the zero value — recursing elementwise through nested arrays and
struct elements while leaving slice, map and pointer elements shared. The gates
are `parity-scripts/array_value_semantics.go` and
`parity-scripts/struct_value_semantics.go`. A `[N]T`'s written type is carried
on the object, so `%T` and `%#v` name the array at every depth
(`parity-scripts/array_type_name.go`).

## `map` operations are O(n) each, so building one is O(n²)

```go
m := make(map[int]int)
for i := 0; i < n; i++ { m[i] = i }
```

| n      | go-rs insert-only | go-rs insert + `range` |
|--------|-------------------|------------------------|
| 4,000  | 0.17s             | 0.44s                  |
| 8,000  | 0.66s             | 1.80s                  |
| 16,000 | 2.62s             | 6.98s                  |
| 32,000 | —                 | 28.32s                 |

Time quadruples when `n` doubles. Every value is correct — this is a cost, not a
divergence — but a map big enough makes a program that finishes instantly under
`go` look hung.

The cause is local to go-rs: `HostObj::Map` is a `Vec<(Value, Value)>` kept in
insertion order (`src/host.rs`), so every lookup, insert and `delete` is a
linear scan comparing keys by value. Insertion order is not incidental — it is
what makes iteration stable, and `fmt` sorts keys when printing regardless — so
closing this means a hash index *beside* the ordered vector rather than
replacing it.

For comparison, ranging a **slice** is linear (32k/64k/128k/256k elements →
0.10s/0.19s/0.39s/0.77s), because a go-rs slice is a `Value::Obj` handle into
the frontend's own heap rather than a `fusevm::Value::Array`, so loading one
copies a `u32` instead of deep-cloning the backing `Vec`.

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

`fmt.Fprintln` / `fmt.Fprintf` / `fmt.Fprint` (writer-directed output),
`strconv.FormatFloat`, and `strings.Builder`'s methods (`WriteString`, `String`)
are rejected at compile time. Each is a build failure rather than a wrong
answer, so a program using one does not run at all.

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

The same missing pass makes a constant expression that *leaves* `int64` range
mid-way wrong even when its result fits:

```go
var e uint64 = 1<<64 - 1
fmt.Println(e)   // go: 18446744073709551615   go-rs: 0
```

Go's constants are arbitrary-precision, so `1<<64` is exact and subtracting 1
lands back inside `uint64`. go-rs folds constants in `i64`, where `1<<64` is
already 0. Writing the value as the decimal literal `18446744073709551615`, or
as `1 << 63`, is correct — only an intermediate that exceeds the width is not.
Closing it needs the constant evaluator to work in a wider (or arbitrary
precision) type, which is the same pass the overflow diagnosis above wants.

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
a `Value` variant. A fixed-size array solved the same erasure the other way, by
stamping its written type on the heap object where the value is born and copying
the tag along with it — which works there because an array is a heap object with
a copy at every site, and does not transfer to a scalar held in `Value::Float`.

## `uint64` loses its signedness through an `any` parameter

```go
var x uint64 = 1 << 63
var a any = x
fmt.Println(x)   // go: 9223372036854775808   go-rs: 9223372036854775808
fmt.Println(a)   // go: 9223372036854775808   go-rs: -9223372036854775808
```

The same erasure, for the same reason. `uint64`, `uint` and `uintptr` share
`Value::Int`'s 64-bit two's-complement bit pattern, so the operations that read
the sign bit (`/`, `%`, `>>`, the ordered comparisons, the conversion to a
float) are emitted unsigned from the static type, and a `fmt` argument is
tagged with its signedness on the way in (`GU64_BOX`) so it prints unsigned —
including through slice elements, map values and struct fields. The tag is
applied from the static type at the call site, so a value that has passed
through an `any`/interface parameter has no width left to read. It wants the
same `Value` variant the `float32` gap above does.

## Two interfaces holding an `int` and a `float64` compare equal

```go
var x any = 1
var y any = 1.0
fmt.Println(x == y)                 // go: false   go-rs: true

var p any = int64(1e18)
var q any = float64(1e18)
fmt.Println(p == q)                 // go: false   go-rs: true
```

Go decides interface equality by dynamic type before value, so an `int` and a
`float64` are never equal however the numbers line up. Comparing two interfaces
is the *only* construct in valid Go that puts the two under one operator:
arithmetic and ordered comparison on mismatched numeric types are compile
errors (`invalid operation: i + f (mismatched types int64 and float64)`), and
interfaces are unordered (`operator < not defined on interface`).

go-rs boxes both as bare `Value::Int` / `Value::Float` with no dynamic type
beside them, and fusevm 0.17.0 answers a mixed pair natively by promoting the
integer to `f64` and comparing numerically — so the pair never reaches
`numeric_hook` and the frontend gets no say. `numeric_hook` already classifies
the pair correctly (`Eq` false, `Ne` true, arithmetic and ordering reported as
mismatched types), which closes the `p == q` half as soon as a fusevm release
delegates a mixed pair past 2^53 rather than rounding it. The `x == y` half
stays open past that release: the values are small, the promotion is exact, and
fusevm has no reason to ask. Closing it needs the dynamic type on the value —
the same representation change the `float32` and `uint64` entries above want.
