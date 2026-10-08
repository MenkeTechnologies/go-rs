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
  cover an input the compiler rejects or one the pinned fusevm answers natively
  before the frontend is asked.

A gap listed here is deliberately **not** represented by a corpus file, because
the corpus is a green byte-parity gate. Close the gap and add the corpus file in
the same change.

## A defined type reached only through a type parameter or a nested value prints bare

```go
type Weekday int
func (d Weekday) String() string { return "Wed" }
func show[T any](v T) { fmt.Println(v) }
show(Weekday(3))                      // go: Wed      go-rs: 3
fmt.Println(struct{ D Weekday }{3})   // go: {Wed}    go-rs: {3}
```

A defined non-struct value is boxed with its type name
(`HostObj::Named`) when it is converted to an interface — assignment,
argument, return, composite element, map key, channel send, `==` against an
interface — and unwrapped where it leaves one for a concrete type. That is
what dispatch, type switches, assertions, `==` and `%T` read, so a `Weekday` in
an `any` behaves as Go's does. Two conversions are not seen: a generic
parameter is erased to its constraint rather than typed as an interface, and a
struct field or map value is formatted by the host, which cannot call a
method. A top-level operand — including a method result of the defined type and an
element of a defined slice of it — and a `[]T` / `[N]T` / `[]any` (or a defined
type over one) are rendered through the method. The same erasure keeps `float32` / `uint64` widths out of an `any`,
and a `*Weekday` is named `main.Weekday` rather than `*main.Weekday`, because
`&x` on a scalar has no address (the entry below).

## A nil pointer in an interface compares equal to nil

```go
type E struct{}
func (e *E) Error() string { return "boom" }
var p *E = nil
var i error = p
fmt.Println(i == nil)   // go: false   go-rs: true
fmt.Printf("%T\n", i)   // go: *main.E go-rs: <nil>
_, ok := i.(*E)         // go: true    go-rs: false
```

An interface value in Go is a (type, value) pair, and it is nil only when both
halves are. A nil `*E` stored in an `error` therefore carries a type and is not
nil — the classic "returned a nil pointer, the caller's `err != nil` fired
anyway" trap, and the one case where getting this wrong changes control flow
rather than output. go-rs represents a nil pointer as `Value::Undef`, which
`go_type_name` maps to `<nil>`, so the pointee type is gone by the time the
comparison, the assertion, or `%T` asks for it.

This is the representation change described two entries above, seen from the
other side: a nil slice keeps its type because it is a `HostObj::Slice`, while
a nil pointer keeps none because it is the same `Undef` every other nil is.
Giving nil pointers a typed representation closes this and the `%T` half of the
defined-type entry together; both are the same missing name-on-the-value.
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

## A call passes at most 255 arguments

```go
fmt.Println(0, 1, 2, /* … 256 arguments in total … */)
// go:    prints them all
// go-rs: compile error — `fmt.Println` takes at most 255 arguments here
```

fusevm 0.26.7's `Op::CallBuiltin` carries its argument count in a `u8`
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

## A comment mentioning a `rust {` block beside a non-ASCII block comment crashes — waiting on a fusevm release

```go
// see the rust { } section below
/* café */
func main() { fmt.Println("ok") }
// go: ok    go-rs: panic in fusevm::rust_sugar (byte index is not a char boundary)
```

Inline Rust blocks are found by `fusevm::RustSugar::desugar`, which steps
through a `/* … */` comment one byte at a time and slices the source at each
step, so a multi-byte character inside a block comment panics. go-rs only
hands it source that has the word `rust` followed by `{` (`rust_ffi::desugar`),
which keeps every ordinary program — a comment saying "trust", Go's own `time`
sources — away from it; a program that does write `rust {`, even inside a
comment, and also has a non-ASCII block comment still reaches the panic.
Closing it needs the scanner to advance by character.

## Range-over-func: a labeled jump out of the body is rejected, and `defer` runs early

```go
outer:
for _, s := range xs {
	for v := range seq {
		if v > 3 { continue outer }   // go-rs: compile error
		defer cleanup()               // go-rs: runs when this yield call returns
	}
}
```

`for … range f` over an iterator function runs the body as `f`'s `yield`
closure (`Compiler::compile_for_range_func`): the loop's own `break` and
`continue` become `return false` / `return true` and a `return` from the
enclosing function is recorded and performed once `f` returns. A `break L` /
`continue L` naming a loop *outside* the range-over-func loop, and a `goto`
out of it, are not carried across the closure boundary; the labeled forms are
refused rather than run wrong. A `defer` in the body belongs to the closure, so
it runs when that iteration's `yield` returns instead of when the enclosing
function does.

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

## A `select` whose send case must block deadlocks — waiting on a fusevm release

```go
out := make(chan int)
go func() {
    for i := 0; i < 3; i++ {
        select {
        case out <- i:          // parks, and is never revisited
        }
    }
}()
for i := 0; i < 3; i++ { <-out }   // go: 3   go-rs: deadlock
```

The order is what decides it. A `select` that runs *after* a peer parked works —
`try_select` reads live channel state and finds the parked sender — which is why
a receive case fed by a blocked sender is fine, and why the corpus file covers
that shape. It is a `select` that parks *first* that is lost.

Two things in `fusevm-0.26.7/src/sched.rs` combine:

- A select's send case does not register in the channel's `send_q`
  (`SchedReq::Select` pushes the goroutine onto `select_waiters` instead,
  `:280`), so a later receiver looking for a partner cannot see it.
- The parking paths do not re-check waiting selects. `SchedReq::Recv`,
  `RecvOk` and `Send` each call `recheck_selects()` only on the branch where
  they *succeeded* (`:245`, `:255`, `:266`); the `else` branch that parks the
  goroutine does not (`:248`, `:258`, `:269`).

So the receiver parks without waking the select, and the select waits for a
receiver it was never told about. Closing it upstream is either half: register
select cases in the channel queues, or re-check waiters when a goroutine parks.
The frontend cannot reach it — `Op::Select` is the only primitive that can wait
on several channels at once, and a two-case cancellation `select` is exactly
what it exists for.

## A nil channel in a `select` aliases the first channel made — waiting on a fusevm release

```go
real0 := make(chan int, 1)
real0 <- 111
var nilch chan int
select {
case v := <-nilch:  fmt.Println("nil-fired", v)   // go-rs takes this case
default:            fmt.Println("default")        // go takes this one
}
```

Go blocks forever on a nil channel, so a `select` over one takes its `default`.
go-rs evaluates the nil to `Value::Undef`, `SchedReq::Select` reads the case's
channel with `to_int()`, and the result is `0` — the id of the *first* channel
the program made. The case is therefore ready whenever that unrelated channel
is, and takes its value: above, `real0` loses the `111` it was holding and the
program then deadlocks waiting for it.

This is the worse of the two, because it is a silent wrong answer rather than a
hang. It needs an id that cannot collide with a real channel — the ids are
indices into the scheduler's `chans` vector, so `0` is a valid one and the
frontend has no sentinel it can emit instead: `try_recv` on an out-of-range id
raises a panic rather than blocking, which is not what a nil channel does.

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

The one place a pointer to a non-struct is made implicitly is closed: calling a
pointer-receiver method on a defined slice or integer type (`func (s *Stack)
Push(v int) { *s = append(*s, v) }`) hands the method a cell holding the
receiver, reads and writes `*s` through it, and stores the result back into the
variable, field or element the method was called on
(`parity-scripts/pointer_receiver_defined_types.go`). The cell lives for the
call, so a method that keeps `s` beyond it sees none of the caller's later
writes.

## A pointer nested inside a printed value prints as the value

```go
p := point{1, "a"}
fmt.Println(outer{p: &p})   // go: {0xc000010030}  go-rs: {{1 a}}
```

A pointer *operand* is right: `&T{…}` / `new(T)` (a `by_ref` handle) and `&x`
(a `HostObj::Ptr`) print as `&{1 a}` / `&[1 2]` / `&map[k:1]` under every verb,
and `%T` names them `*main.T` (`parity-scripts/pointer_operand_printing.go`).
One level down Go prints the pointer's hex address instead, which no two runs
reproduce, so go-rs keeps printing the value there rather than inventing an
address.

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

## `%T` in a computed format string names the rendered type

```go
var v any = myErr{"e"}       // myErr has an Error() string method
f := "%T\n"
fmt.Printf(f, v)             // go: main.myErr    go-rs: string
```

`fmt` reaches `Error()`/`String()` through the linker-synthesized `$stringify`,
which the compiler wraps around an operand only when its verb prints text. For
a literal format the compiler reads each operand's verb, so `%T`, `%d` and
`%#v` see the operand itself. A format held in a variable is only known at run
time, and every operand is then wrapped as `%v` would need — so `%T` sees the
`string` the method returned.

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

## `fmt.Errorf` used as a value panics

```go
ef := fmt.Errorf            // go: an error-building func   go-rs: panic: nil pointer dereference
```

Every other natively implemented stdlib function is a value: `pkg::stdlib_func_value`
writes a literal of its signature that forwards to the call
(`func(a ...any) (int, error) { return fmt.Println(a...) }`), and the selector
lowers to it (`parity-scripts/stdlib_func_value.go`). `fmt.Errorf` is left out
because the forward would be wrong rather than missing: `Errorf` picks its
result type (plain, `*wrapError`, `*wrapErrors`) by counting the `%w` verbs in
its format at compile time, and inside the forwarder the format is a parameter,
so `%w` would wrap nothing. Closing it needs `Errorf`'s wrap decision moved to
run time.

`%T` of any function value — a literal, a declared function or one of these —
prints `func()`, not its signature.

## The heap is never collected, so a print in a loop retains its result

`heap_alloc` only pushes; nothing frees, and `heap_reset` runs once per program.
Every heap value a program builds is retained for the run, so a loop that builds
one per iteration grows without bound. 200k iterations of `fmt.Fprintln(os.Stdout,
"x")` — which returns `(n, err)` through the writer's `Write` — peak at 99 MB
against 11.5 MB for the same loop as `fmt.Println("x")`, about 440 bytes an
iteration.

This is why `fmt.Print`/`Printf`/`Println` have two builtin ids each: the
statement form answers `Undef` and allocates nothing, and only a print whose
result is read builds the pair (`Compiler::discard_print`). That keeps the
common case free but does not address the general problem — any loop over
`strconv.Atoi`, `append`, or a struct literal has the same shape.

## Unsupported stdlib calls

Writer-directed output is implemented. `fmt.Fprint` / `Fprintf` / `Fprintln`
rewrite to `w.Write([]byte(fmt.Sprint*(…)))`; `io` and `bytes` are vendored
(`goroot/io.go`, `goroot/bytes.go`); and `strings.Builder` and `os.Stdout` /
`os.Stderr` are synthesized and qualified under their native packages
(`goroot/strings_builder.go`, `goroot/os_file.go`).

What is still missing from that corner:

- **`Cap` / `Grow`** on `strings.Builder` and `bytes.Buffer` are deliberately
  absent: go-rs cannot reserve capacity, so answering would mean answering
  wrongly. (`Builder.Grow` is a no-op, which nothing can observe once `Cap` is
  gone.) A program calling `Cap` gets a compile error.
- **`bytes.Buffer`'s read half** — `Read`, `ReadString`, `Next`, `UnreadByte`
  and the read offset they share. The write half is what a program reaches for
  when it wants somewhere to write, and is what is vendored.
- **`bytes`'s `*Func` trimmers and the rarer functions** — `TrimFunc`,
  `TrimLeftFunc`, `TrimRightFunc`, `LastIndexFunc`, `FieldsFunc`,
  `LastIndexAny`, `CutLast`, `Title`, `ToValidUTF8`, the `*Special` case
  mappings and the `iter` sequences (`Lines`, `SplitSeq`, …) are not vendored;
  a call is a compile error. The common package functions are
  (`parity-scripts/bytes_package_funcs.go`).
- **`bufio` is only its `Writer`.** `Reader` and `Scanner` read from an
  `io.Reader`, which go-rs's `io` does not have, and there is no `os.Stdin`
  to scan.
- **`os.File` is only the two standard streams.** There is no `Open`, `Create`
  or `Read` — `writeFd` is the package's one intrinsic and only ever sees
  descriptors 1 and 2.

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

The same missing pass leaves one corner of exact constant arithmetic open. A
`const` declaration and a literal-only expression are folded exactly (in
`i128`) whenever an `i64` step would wrap, so `const big = 1<<64 - 1`, `half =
big >> 1`, `var e uint64 = 1<<64 - 1` and Go's own `math/bits` are right
(`parity-scripts/const_exact_uint64.go`). What is not folded is an expression
outside a `const` declaration that *names* a constant above `int64`:

```go
const big = 1<<64 - 1
fmt.Println(uint64(big >> 4))            // go: 1152921504606846975   go-rs: 18446744073709551615
fmt.Println(uint64(math.MaxUint64 >> 1)) // go: 9223372036854775807   go-rs: 18446744073709551615
```

A constant is lowered as a variable, so at that point `big` is a run-time
`uint64` bit pattern with no static type to make `>>` logical. Folding it needs
the compiler to know which names are constants — through block scoping and
shadowing — which is the same pass the overflow diagnosis above wants.

## Transcendental `math` functions differ from Go in the last bit

```go
for i := 0; i < 20000; i++ {
	v := -700 + float64(float64(i)*0.0713)
	fmt.Println(math.Pow(1.0001, v), math.Sin(v), math.Atan(v))
}
// go-rs differs from go 1.27.1 (darwin/arm64) in the last printed digit on
// 19894 of the Pow lines, 5452 Sin, 5531 Atan, 7846 Hypot, 5044 Log10, ...
```

`Sin`, `Cos`, `Tan`, `Asin`, `Acos`, `Atan`, `Atan2`, `Sinh`, `Cosh`, `Tanh`,
`Exp`, `Log`, `Log2`, `Log10`, `Pow`, `Cbrt` and `Hypot` are host builtins over
Rust's `f64` methods, which call the platform's libm. Go computes them with its
own algorithms (`math/sin.go`, `pow.go`, …), so the two agree only to within an
ulp or so. `Sqrt`, `Floor`, `Ceil`, `Trunc`, `Abs`, `Mod` and `Copysign` are
exact in both and agree. Closing it means porting Go's algorithms; for byte
parity the port also has to fuse `x*y + z` into one rounding wherever Go's
compiler does, which on arm64 (and not on amd64) is every such expression —
the same reason a user's own `a*b + c` can differ from Go on arm64.

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
three are "the static type at the conversion, readable at run time".

**Re-assessed after the pointer object landed, and it is materially more
tractable than this entry used to claim.** Two of the three obstacles are gone:

- The box already exists. `HostObj::Named { ty, inner }` carries a type name
  beside a value, with `unname` to read through it, and `GNAMED_BOX` emits it —
  today only at a `fmt` argument position.
- The conversion site now exists. The entry said `var x any = 1` compiles to
  `LoadInt(1); SetVar(0)` with nowhere to hook. It no longer does: every binding
  site was routed through `Compiler::emit_typed` when untyped constants were
  taught to convert to a float destination — a var, an argument, a struct field,
  a container element, a channel send, a map key. `is_iface_ty` is already there
  to test the destination.
- The pointer object is the worked example that the readers are few, not 74. A
  `Ptr` needed resolving at eight sites, because it can only appear where a
  pointer can. A `Named` int is the same: valid Go requires a type assertion
  before any arithmetic on an interface value, so the box reaches only the
  interface operations — `go_type_name`, `iface_eq`, `key_eq` / `map_key`, the
  assertion and type switch, and printing.

What is left is real work rather than a blocker: `key_eq` and `map_key` must
carry the name (a boxed key and a plain one of the same number are otherwise two
keys), and the unbox has to happen at every assertion. It wants its own change
with the full fuzz behind it, not a corner of another one.

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

## Rebinding through a pointer to a slice

```go
var s []int
p := &s
*p = append(*p, 1)          // go-rs: cannot assign through a pointer to a
                            //        non-composite value
```

*Reading* through a `*[]T` or a `*map[K]V` is fixed — `*p` follows the
`HostObj::Ptr` to the slice or map, so `len`, `cap`, indexing (read and write),
`range`, `delete`, `append`'s operand and `*p == nil` all see the value
(`parity-scripts/pointer_to_slice.go`,
`parity-scripts/pointer_to_map_and_nil_deref.go`). Writing a new slice through
the pointer does not: `b_deref_set` overwrites the *pointee in place*, which is
right for a struct and wrong for a slice: a slice rebind has to change the
header the variable holds, and go-rs models a slice as one object, so an
in-place overwrite would also be seen through a `t := s` taken earlier, which
Go leaves alone. Making it right needs `&s` to address the variable's storage
rather than the slice object — the same change the scalar `&x` entry above
wants. The accumulator idiom
`func add(out *[]int, v int) { *out = append(*out, v) }` is what reaches it,
and so is every `container/heap` `Push` / `Pop`, whose pointer receiver on a
slice type is called through the `heap.Interface`: a direct `h.Push(x)` hands
the method a cell for the receiver, but a call through an interface has no
variable to hand back.

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

Go's own `math/bits` meets it in `Reverse8` and `Reverse16`, which index a
string constant of bytes (`rev8tab`) — `bits.Reverse8(1)` is `194` (the first
byte of U+0080's encoding) where Go answers `128`.

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

Re-assessed: the `%T` hazard that made this look worse than it is can be
designed out. Mangle only on an actual collision — a local type name declared
once in the whole program, which is every existing program, keeps its name and
its `%T` output untouched, and only the second declaration of a shared name
needs a display name beside its mangled one. What remains is the part that was
always the work: a rename walker over one function body's *type positions*,
where a reference may precede the declaration. It is tractable and it is the
lowest-value item on the list — the collision needs two same-named local types
in one program to be observable at all.

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

## The zero value of a type parameter is `nil`

```go
func first[T any](xs []T) T { var zero T; if len(xs) == 0 { return zero }; return xs[0] }
fmt.Println(first([]int{}))          // go: 0        go-rs: <nil>
next, stop := iter.Pull(seq)         // after the sequence ends:
v, ok := next()                      // go: 0 false  go-rs: <nil> false
```

Generics are erased: one body runs for every instantiation, so inside it `T`
names no type and `var zero T` has nothing to be the zero *of*. go-rs makes it
`nil`, which arithmetic and string concatenation treat as the identity (so a
generic sum or join is right) but which prints as `<nil>`. The vendored
`iter.Pull` / `Pull2` return that zero once the sequence is over, the same way
Go's do. Closing it needs the instantiation's type arguments at run time —
monomorphizing, or passing the type arguments as hidden parameters.

## `%T` of an instantiated generic type omits its type arguments

```go
type Pair[K comparable, V any] struct{ Key K; Val V }
fmt.Printf("%T\n", Pair[string, int]{"a", 1})   // go: main.Pair[string,int]
                                                // go-rs: main.Pair
```

Type parameters are erased: one `Pair` struct type serves every
instantiation, and its run-time name is what method dispatch, type switches
and the struct copy plan are keyed by. Naming the instantiation means carrying
the type arguments on the value — known at a composite literal outside a
generic body, but only as type parameters inside one — and stripping them
wherever the erased name is the key.

## A rune literal is an untyped integer

```go
r := 'a'
fmt.Printf("%T\n", r)   // go: int32   go-rs: int
```

The lexer folds a rune literal to its integer value, so nothing downstream
knows it was a rune: a variable it initializes is typed `int`. A rune that
gets its type from a declaration (`var r rune = 'a'`, a `[]rune` element, a
`range` over a string) is `int32` as in Go.
