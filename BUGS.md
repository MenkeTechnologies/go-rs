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
- `cargo test` — for the cases neither of the above can reach from a `.go` file.
  A rule's decision function is called directly there, which is the only way to
  cover an input the compiler rejects (`NaN`, since `math.NaN` is a rejected
  stdlib call) or one the pinned fusevm answers natively before the frontend is
  asked.

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

## Spawning a goroutine copies the whole program — waiting on a fusevm release

```go
for i := 0; i < n; i++ { wg.Add(1); go func() { defer wg.Done(); … }() }
```

| goroutines | program (lines) | go-rs  |
|------------|-----------------|--------|
| 500        | 19              | 0.03s  |
| 500        | 419             | 0.11s  |
| 500        | 1,619           | 0.58s  |
| 1,000      | 19              | 0.06s  |
| 1,000      | 1,619           | 1.18s  |

Spawn cost is linear in *program size*, so a big program's goroutines are
expensive for no reason of their own. Every value is correct — this is a cost,
not a divergence.

`sample` on 12,000 goroutines in the 1,619-line program says where it goes:

```
  1404  alloc::slice::…to_vec_in::<fusevm::op::Op>
   919  core::slice::iter::Iter<fusevm::op::Op>::…
   704  <fusevm::op::Op as core::clone::Clone>::clone
   630  core::iter::adapters::Enumerate<slice::Iter<…>>::…
   585  _platform_memmove
   222  core::slice::iter::Iter<(u16, usize)>::find
   210  fusevm::chunk::Chunk::find_sub::{closure}
```

That is a deep copy of `Chunk` per goroutine — `ops`, `lines`, `names` and the
constant pool — and it is about three quarters of the run. (Registering the
builtins per goroutine does *not* show up; the copy is the whole story.)

It cannot be closed from the frontend. `fusevm::Scheduler::new` takes an
`FnMut() -> VM` factory and keeps every goroutine's `VM` in `vms: Vec<VM>`
across suspension, while `VM::new` and `VM::reset` both take `Chunk` **by
value** — so there is no way to hand the scheduler a VM that shares the program
rather than owning a copy of it, and no way to pool and reset one either (a
suspended goroutine still holds its VM). Closing it needs a fusevm release
where the VM borrows or `Arc`-shares its chunk; the published pin is the
contract.

## One slice in a frame keeps every loop in that function interpreted — waiting on a fusevm release

```go
func f(s []int, n int) int {
	t := 0
	for i := range n { t = (t + i) % 1000003 }   // never traced
	return t
}
```

Every Go loop form go-rs emits is now shaped for fusevm's tracing JIT — the
three-clause `for`, the condition-only `for`, `for {}` and `range` over an
integer all report `traced=true` under `go --tiers`, and `src/tiers.rs` asserts
each of them. The loop above is *identical* to the one asserted there, and
reports `trace-eligible=true traced=false`, because the enclosing function also
has a slice parameter.

`VM::refresh_slot_buffers` classifies a frame's slots and sets one
`slots_all_numeric` flag for the **whole frame**;
`lookup_trace_for_backward` returns the anchor unentered whenever that flag is
false and a numeric hook is installed — which go-rs always installs, because
Go's fixed-width overflow is what it decides. A slice, map, string or struct is
a `Value::Obj`, so one of them anywhere in the frame keeps every loop in that
function in the interpreter however numeric the loop itself is.

It cannot be closed from the frontend: a Go program cannot keep its composite
values out of its frames. fusevm already does the finer thing for globals
(flagged per index, with the trace's entry guard refusing only on the indices it
reads) and already knows which slots a trace touches
(`jit::collect_trace_slots`), so the per-slot version is a change in the same
file — but the published pin is the contract. Every value is correct either way;
this is a cost, not a divergence.

The other loop form that stays interpreted is `range` over a **slice, map or
string with a value variable**: the body has to call `GRANGE_VAL` to fetch the
element, and `jit::is_trace_op_allowed_at` refuses any `Op::CallBuiltin`
outright. That one is representational — a Go slice is a handle into go-rs's own
heap, not a `fusevm::Value::Array`, so there is no native op that can read an
element.

## `&x` on a scalar has no address, so two pointers to equal values compare equal

```go
p1, p2 := 1, 1
fmt.Println(&p1 == &p2)   // go: false   go-rs: true
```

go-rs models `&x` on a non-struct as the value itself, so a `*int` carries no
identity to compare: `==` falls through to the value, and two distinct pointers
to equal values are one key of a `map[*int]V` rather than two. A pointer *field*
inside a struct key inherits the same merge.

`&x` on a **composite** is no longer affected: it allocates a `HostObj::Ptr`
addressing the variable's handle, so it binds, stores and compares as a pointer.
That machinery cannot reach a scalar for the reason the gap exists — an `int` is
not a heap object, so `GPTR_TO` has nothing to point at and passes the value
through. `*p = v` through such a pointer is refused rather than answered:

```go
a := 5
pa := &a
*pa = 9   // go-rs: cannot assign through a pointer to a non-composite value
```

Closing it means boxing any scalar whose address is taken — a heap cell of its
own, which the closure-capture path already builds for a different reason — and
teaching `*p` to read through one. That changes what every `&x` and `*p` on a
scalar costs, and every loop holding such a variable would leave the tracing
tier, so it is a deliberate trade rather than an oversight.

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
// go-rs: panic: interface conversion: main.errors.errorString is not
//        interface{Unwrap/0:error}: missing method Unwrap
```

Whether the assertion succeeds is right — that is method-set containment on
signatures, and it is what `errors.Is`/`As` rely on. Only the panic *text*
differs, because the parser canonicalizes an inline interface to
[`method_sig`]-encoded names (`src/ast.rs`) rather than keeping the written
source, which it cannot recover: tokens carry a line but no byte offset.

A *named* interface is not affected — `x.(Stringer)` panics with Go's exact
`interface conversion: int is not main.Stringer: missing method String`, and so
does `x.(error)`. The synthesized `errors`/`fmt` error types keep the `main.`
qualifier of the one package go-rs compiles, and lose the `*` for the reason in
the pointer entry above.

## Unsupported stdlib calls

Writer-directed output is implemented. `fmt.Fprint` / `Fprintf` / `Fprintln`
rewrite to `w.Write([]byte(fmt.Sprint*(…)))`, `io` is vendored
(`goroot/io.go`: `Writer`, `StringWriter`, `WriteString`), and
`strings.Builder` and `os.Stdout` / `os.Stderr` are synthesized and qualified
under their native packages (`goroot/strings_builder.go`, `goroot/os_file.go`).

What is still missing from that corner:

- **`bytes.Buffer`** — the same shape as `strings.Builder`, and `bytes` is not
  native, so it is the ordinary vendored-source path rather than a synthesis.
- **`strings.Builder.Cap`** is deliberately absent: go-rs cannot reserve
  capacity, so answering it would mean answering wrongly. `Grow` is a no-op,
  which nothing can observe once `Cap` is gone. A program calling `Cap` gets a
  compile error.
- **`os.File` is only the two standard streams.** There is no `Open`, `Create`
  or `Read` — `writeFd` is the package's one intrinsic and only ever sees
  descriptors 1 and 2.

`strconv.FormatFloat` is implemented for the `f`, `F`, `e`, `E`, `g` and `G`
verbs at both `bitSize`s, including `prec == -1`. The two remaining verbs —
`b` (binary exponent) and `x`/`X` (hexadecimal float) — fault at run time
rather than answering, because the alternative is a decimal string presented as
a hex-float one.

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

## Two interfaces holding two *integer widths* compare equal

```go
var x any = 97
var y any = int64(97)
fmt.Println(x == y)                 // go: false   go-rs: true

var b any = byte(97)
var r any = rune(97)
fmt.Println(b == r)                 // go: false   go-rs: true
```

Go decides interface equality by dynamic type before value, so two interfaces
holding different types are never equal however the numbers line up. That rule
is implemented — `iface_eq` (`src/host.rs`) compares `go_type_name` before the
value, and the compiler routes an `==`/`!=` with an interface-typed operand to
`GIFACE_EQ` rather than emitting the native op. It decides every crossing go-rs
can see at run time: `any(1) == any(1.0)` is false, so is `any(1) == any("1")`,
so are two struct types with the same field, and an interface holding a *typed*
nil (a nil slice or map) is not equal to `nil`. The gates are
`parity-scripts/iface_equality.go`, `tests/iface_equality.rs`, and shape 38 of
the fuzzer.

What is left is the crossing the values cannot show. `int`, `int8`…`int64`,
`uint`…`uint64`, `byte` and `rune` are all one `Value::Int`, and
`go_type_name` names every one of them `int`, so two of different width look
like the same dynamic type and are compared by value.

The same crossing shows as a **map key**, and only that crossing now:

```go
m := map[any]string{}
m[1] = "int"
m[int64(1)] = "int64"
fmt.Println(len(m))                 // go: 2   go-rs: 1
```

`key_eq` (`src/host.rs`) partitions keys into nil, string, bool, integer and
float, so `map[any]V` keeps a `"1"`, a `true`, a `nil`, a `1` and a `1.0` apart.
It could not do the integer/float half until the *compiler* stopped letting an
untyped constant reach a float destination as a `Value::Int`
(`Compiler::emit_map_key` and the argument, field, element, `append` and channel
-send conversions beside it). Two integer widths remain one key, for the same
reason two of them compare equal above: the width is in the static type.

Closing it needs the Go type on the value, which is a representation change,
and it is the *same* one the `float32` and `uint64` entries above want — all
three are "the static type at the conversion, readable at run time". fusevm
0.17.0's `Value` has no spare tagged-scalar variant (`Undef`, `Bool`, `Int`,
`Float`, `Str`, `Array`, `Hash`, `Status`, `Ref`, `NativeFn`, `Obj`), so the
tag has to ride on `Value::Obj` — a heap handle, exactly the shape
`HostObj::F32` and `HostObj::U64` already are. Two things make it large rather
than a patch:

- There is no concrete-to-interface conversion site to hook. `var x any = 1`
  compiles to `LoadInt(1); SetVar(0)` and nothing else, so the box would have to
  be emitted at each of them separately: a var declaration, an assignment, an
  argument bind (including `...any`), a return, a slice or array literal, a map
  key and a map value, a struct field, a channel send, and `append`.
- Every reader has to see through it. That is 74 host builtins, plus `go_str`
  and `go_type_name`, plus map-key equality — a `GoMap` is ordered pairs plus a
  hash index, both keyed on `Value` equality, so a boxed key and a plain one of
  the same number would be two different keys.

What makes it *feasible* rather than impossible is that valid Go requires a type
assertion or a type switch before any arithmetic on an interface value, so the
box can never reach a native fusevm arithmetic op. The one exception was `==`
and `!=`, which used to be emitted as `Op::NumEq`/`Op::StrEq` — and that is the
site `GIFACE_EQ` now owns.

## Two interfaces holding a slice, map or func do not panic

```go
var a any = []int{1}
var b any = []int{1}
fmt.Println(a == b)
// go:    panic: runtime error: comparing uncomparable type []int
// go-rs: true
```

Go's `==` on interfaces is defined only when the dynamic type is comparable, and
panics at run time when it is not. go-rs compares the two structurally and
answers instead. `iface_eq` gets the dynamic *type* right here — both are
`[]int` — so what is missing is the comparability table that says a slice, a map
and a func have no `==`, which is the same static-type knowledge the entry above
wants on the value.

## A variadic closure binds its parameters wrong

```go
p := func(f string, a ...any) { fmt.Println(f, len(a)) }
p("c0")
p("c1", 1)
// go:    c0 0 / c1 1
// go-rs: <func> 2 / c1 0
```

A call through a *func value* is dispatched by `Op::CallDynamic`, which pushes
one stack slot per written argument. Only a call to a named function packs the
trailing arguments into the variadic slice parameter (`Compiler::call`, the
`self.funcs.get(name)` arm), so a closure receives the wrong number of operands
and every parameter binds one slot off — the closure handle itself lands in the
first parameter.

Closing it needs the variadic bit to survive to the call site, and it currently
does not exist past the parser: `Expr::FuncLit` (src/ast.rs) carries `params`,
`results` and `body`, but no `variadic` flag, even though `Parser::params`
computes one. So the work is (a) add `variadic` to `Expr::FuncLit` and to
`LambdaInfo`, (b) record each func-literal binding's signature the way
`self.funcs` records a declared function's, and (c) pack at the `CallDynamic`
site from that signature. Spreading into one (`g(xs...)`) is the same gap seen
from the other side.

## A string cannot hold invalid UTF-8

```go
s := "aé中z"
c := s[1:2] // the first byte of a two-byte code point
fmt.Println(len(c))
fmt.Printf("%q %x %d\n", c, c, c[0])
// go:    1 / "\xc3" c3 195
// go-rs: 3 / "�" efbfbd 239
```

A Go string is an arbitrary byte sequence and a slice expression cuts bytes, so
a cut that lands inside a code point yields a string that is not valid UTF-8.
go-rs holds a string as fusevm's `Value::Str` — a Rust `String`, which is
UTF-8 by construction — so the cut is replaced lossily with U+FFFD, and `len`,
`%q`, `%x` and the byte read all answer about the replacement instead.

This is a representation gap, not a formatting one: closing it means a byte
string in the shared VM (`Value::Str` is fusevm's, used by every frontend), so
it is not a change go-rs can make alone.

## A `type` declaration inside a function body is not scoped to its block

```go
func a() { type T struct{ X int } }
func b() { type T struct{ Y string } }   // go: two types   go-rs: one, from `a`
```

A body-local `type` is parsed and hoisted to the program's declarations, which
is where the compiler reads every type from. That is right for everything the
name is observable through — `%T` prints `main.T` whether the declaration was
local or package-level — and wrong only when two blocks declare *different*
types under one name, where the first one parsed wins. A local declaration that
shadows a package-level name has the same collision.

Closing it needs a scope chain in the parser and mangled names in the program,
so that `T` in `a` and `T` in `b` are two entries. The gate for what does work
is `parity-scripts/local_type_decl.go`.

## `%w` outside `fmt.Errorf` renders instead of reporting

```go
fmt.Printf("%w\n", errors.New("gone"))
// go:    %!w(*errors.errorString=&{gone})
// go-rs: gone
```

`%w` is `fmt.Errorf`'s wrap verb; `Printf` rejects it. go-rs lowers `Errorf` to
the same `Sprintf` (reading `%w` off the format separately to pick the wrap
type), so the one format path has to accept `%w` and render it as `%v`. Keeping
`Errorf` correct is worth more than rejecting a verb that is only ever written
inside it. Separating them means giving `Errorf` its own formatter entry point.
