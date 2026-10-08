```
 ██████╗  ██████╗       ██████╗ ███████╗
██╔════╝ ██╔═══██╗      ██╔══██╗██╔════╝
██║  ███╗██║   ██║█████╗██████╔╝███████╗
██║   ██║██║   ██║╚════╝██╔══██╗╚════██║
╚██████╔╝╚██████╔╝      ██║  ██║███████║
 ╚═════╝  ╚═════╝       ╚═╝  ╚═╝╚══════╝
```

[![CI](https://github.com/MenkeTechnologies/go-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/MenkeTechnologies/go-rs/actions/workflows/ci.yml)
![Rust](https://img.shields.io/badge/Rust-2021-05d9e8?style=flat-square)
[![Docs](https://img.shields.io/badge/docs-online-blue.svg)](https://menketechnologies.github.io/go-rs/)
![license](https://img.shields.io/badge/license-MIT-ff2a6d?style=flat-square)
![status](https://img.shields.io/badge/status-active%20%C2%B7%20in%20development-9b5de5?style=flat-square)

### `[GO, COMPILED TO BYTECODE — JIT-COMPILED, NO GO TOOLCHAIN]`

> *"No goroutine scheduler to warm, no garbage collector to tune. go-rs lowers Go to bytecode and lets the JIT run it."*

**Go in Rust** — a Go frontend hosted on the
[`fusevm`](https://github.com/MenkeTechnologies/fusevm) bytecode VM with a
three-tier Cranelift JIT — the same engine behind `zshrs`, `strykelang`,
`awkrs`, `vimlrs`, `elisprs`, `rubylang`, `javars`, `kotlinrs`, and `scalars`.
No `go` toolchain, no `gc` compiler, no runtime.

go-rs is a **pure frontend**: it lexes Go (with the language's automatic
semicolon insertion), parses it, and lowers the AST straight to `fusevm::Chunk`
bytecode. There is no bespoke interpreter loop — execution and code generation
are the shared fusevm engine. Go's `+` string-concatenation overload and string
ordering are dispatched through fusevm's strict numeric hook, which also wraps
overflowing integer arithmetic and settles nil and mixed `int`/`float64`
identity.

## Pipeline

```
Go source
   │  lexer.rs      — tokens + automatic semicolon insertion (ASI)
   ▼
tokens
   │  parser.rs     — recursive-descent → Go AST
   ▼
ast::Program
   │  scope.rs      — rename block-shadowed locals apart (Go block scoping)
   ▼
ast::Program
   │  compiler.rs   — lower to fusevm ops (LoadInt, Add, Call, JumpIfFalse, …)
   ▼
fusevm::Chunk
   │  fusevm        — three-tier Cranelift JIT + host builtins (host.rs)
   ▼
output
```

## Usage

```sh
go run file.go        # compile and run a Go program on fusevm
go file.go            # shorthand for `go run`
go build -o bin f.go  # AOT-compile to a standalone native executable (no go toolchain)
go vet file.go        # parse + compile-check; report errors, do not run
go env                # print the Go environment (GOOS/GOARCH/GOVERSION/…)
go doc [name]         # reference docs for a keyword/type/builtin (or the index)
go install-std        # install the vendored standard library into ~/.go-rs
go version            # print the version banner
go help [command]     # usage (optionally for one command)
go --dump-tokens f.go # lexer token stream (with inserted semicolons)
go --dump-ast f.go    # parsed AST
go --disasm f.go      # lowered fusevm bytecode
go --tiers f.go       # run it, then report which fusevm tiers took it
go --lsp / --dap      # Language Server / Debug Adapter Protocol over stdio
```

`go build` emits a native binary via fusevm's AOT object emitter linked against
the go-rs runtime — it runs with no `go` toolchain and no go-rs. (Concurrency
programs need the scheduler, so goroutine/channel/`select` code uses `go run`.)
go-rs is an **executor swap**: it runs Go on fusevm instead of the `go`
toolchain's runtime. The standard library is implemented **natively in Rust**
(host builtins) and grows package by package; importing a package go-rs hasn't
implemented yet is a clear error rather than a silent miss.

### Example

```go
package main

import "fmt"

func fib(n int) int {
	if n < 2 {
		return n
	}
	return fib(n-1) + fib(n-2)
}

func main() {
	for i := 0; i < 10; i++ {
		fmt.Println(fib(i))
	}
}
```

```sh
$ go run fib.go
0
1
1
2
3
5
8
13
21
34
```

More programs live in [`examples/`](examples).

## Language surface

Real Go, executed on fusevm:

| Area           | Supported                                                              |
| -------------- | --------------------------------------------------------------------- |
| Declarations   | `package`, `import` (single + grouped), `type T struct` / `interface` / defined (`type Weekday int`) — single or grouped `type ( … )`, at package level **or inside a function body** — top-level `func` and methods (`func (r T) m()`) |
| Variables      | `:=`, `var x [T] [= e]` and the multi-name forms (`var a, b int = 1, 2`, `var a, b = f()`, `var a, b int`), assignment to lvalues (ident / `x[i]` / `x.f`), parallel assignment `a, b = x, y` (swap/rotate; RHS evaluated first), `a, b = f()`, `+= -= *= /= %=`, `x++` / `x--`. All three comma-ok forms — `m[k]`, `x.(T)`, `<-ch` — take `=` into existing variables as well as `:=`, and into any assignment target (`b.OK, s[i], out["x"], _`) |
| Control flow   | `if` / `else if` / `else` (with init clause), three-clause / condition / infinite `for`, `for … range` (over slices, arrays, strings, maps, channels, an integer `range n`, and an iterator function — Go 1.23 range-over-func, the body running as the `yield` callback with `break` / `continue` / `return` carried out of it), `switch` (tagged / expression / multi-value cases / init clause / `fallthrough`) and type switch, `break`, `continue`, `return`, **labeled** `break L` / `continue L` naming an enclosing `for` or `switch`, and `goto L` to a label on any statement of the same function (forward or backward; an undefined, duplicate or unused label is a build error, as in Go) |
| Expressions    | int / float / string / bool literals (incl. `0x` / `0o` / `0b` bases, `_` separators, and uint64 masks above `i64::MAX` stored by bit pattern), rune literals as int32 code points (`'A'` == 65, `'z' - '0'`) with the full escape set (`\n \t \xHH \uHHHH \UHHHHHHHH` + octal, in rune **and** string literals), arithmetic, bitwise `& \| ^ << >> &^` (+ `^x` complement, compound `&= \|= ^= <<= >>= &^=`), comparisons, `&&` `\|\|` `!` (short-circuit), unary, parentheses, calls, recursion |
| Types          | `int` family, `float32/64`, `string`, `bool`, defined types over any base (`type Celsius float64`, `type mySlice []int`, `type myMap map[string]int`) — a distinct type with its base's representation, so `Celsius(x)` converts, a method declared on it dispatches, a `mySlice{…}` literal is the base's, and `%T` / `%#v` print `main.Celsius`; tracked statically so `int / int` truncates and `float / float` stays exact, and so a `float32` expression is computed *at 32-bit width* and printed with the shortest decimal that round-trips at 32 bits; conversions `T(x)` (`int(f)`, `float64(n)`, `string(rune)`, `byte`/`rune`/…), conversion to an interface type (`error(e)`, `any(x)`, `fmt.Stringer(x)`, a declared `I(x)`, and every implicit one — a defined value keeps its type name inside the interface, so dispatch, type switches, `==` and `fmt`'s `String()` / `Error()` see it), and slice conversions `[]byte(s)` / `[]rune(s)` (and `string([]byte)` / `string([]rune)` back) |
| Constants      | `const x = …` and grouped `const ( … )` blocks with `iota` (auto-increment, expression repetition, `1 << iota` flag patterns); an integer constant sizes an array (`[N]T`, `[N*2]T`, `type d [MaxCase]rune`), declared before or after the use |
| Initialization | package-level variables initialize in **dependency order** — the earliest declaration whose initializer (including, transitively, the bodies of the functions it calls) depends on nothing uninitialized — then every `func init()` runs once, in source order, before `main` |
| Slices         | `[]T{…}`, `make([]T, n)` / `make([]T, n, cap)` (spare capacity is real backing-array room; a pointer, interface, func, channel, slice or map element starts as nil), `s[i]`, `s[i] = v`, slice expressions `s[lo:hi]` / `s[:hi]` / `s[lo:]` / three-index `s[lo:hi:max]` (the capacity bound is applied: the result's `cap` is `max - lo`, so a later `append` reallocates instead of clobbering the parent; two-index also on strings) that **share the backing array** (writes alias the parent; a re-slice is bounded by `cap`, not `len`; `append` writes in place when the backing has room, else reallocates by Go's `runtime.nextslicecap` growth so `cap` doubles the way Go's does), `len` / `cap` / `append` (including Go's one non-slice spread, `append(b, s...)` appending a string's bytes to a `[]byte`), `for i, v := range s`, every read through a pointer to a slice (`len(*p)`, `(*p)[i]` for read and write, `range *p`, `append(*p, x)`), `for i := range n` over an int (Go 1.22); a nested element type may be elided inside a literal (`[][]int{{1, 2}}`, `[]T{{…}}`); ranging a **string** yields runes (byte offset + code point, once per rune) |
| Arrays         | fixed-size `[N]T` / `[...]T`: sequential `[3]int{…}`, sparse index-keyed `[N]T{3: v}` with zero-fill, elided element literals (`[2][2]int{{1, 2}, …}`, `[N]T{{…}}`), and bare `var buf [N]T` zero-filled to N element zeros. An array is a **value**, like a struct and unlike a slice: it is copied — elementwise, so nested arrays and struct elements separate at every depth, while slice/map/pointer elements stay shared — on assign, argument bind, return, container store and read, `append` (including a spread), channel send, and `range` (which walks a copy, so a write inside the loop is not seen by the remaining iterations). `==` compares elementwise, which makes an array a usable map key (`m[[2]int{1, 2}]`); `a[:]` yields a slice over that array's storage |
| Maps           | `map[K]V{…}`, `make(map[K]V)`, `m[k]`, `m[k] = v`, `delete`, `len`, `for k, v := range m`; element types may be elided inside a literal (`map[string][]int{"a": {1, 2}}`). Pairs are kept in insertion order — which is the order `range` walks — beside a hash index over the keys, so lookup, insert and `delete` are constant-time; a struct or array key hashes structurally, the same way it compares. A **missing key yields the value type's zero** (`""`, `false`, a nil slice, a nil pointer, a zero struct), and `v, ok := m[k]` yields that same zero beside `false`. A key type Go rejects as not comparable (`[]T`, `map[K]V`, `func`, or a struct/array built from one) is a compile error here too. The zero value is a **typed nil** — it prints `map[]`, reads as empty, is `== nil`, and panics `assignment to entry in nil map` on a write, exactly as Go's does (a nil slice is the same: `[]`, `len` 0, appendable) |
| Structs        | `type T struct{…}`, literals `T{…}` / `T{f: v}`, field read/write `s.f` (a func-typed field is called as the value it holds, `p.stage(8)`), **value-copy semantics** — transitive through nested struct fields — on assign, argument bind, return, container store and read, `range` binding, `append`, channel send and value-receiver calls, while pointer/slice/map fields stay shared; **embedded fields** (`struct { Base }`, including `*Base`) whose fields and methods are **promoted** onto the outer type through any depth of embedding (an embedded *interface* promotes its method set, so `struct { sort.Interface }` overriding one method works) — an outer declaration shadows a promoted one, and a promoted method satisfies an interface |
| Methods        | value/pointer receivers (named or unnamed — `func (T) m()`), `recv.m(args)` dispatch by receiver type; **method values** (`f := q.Area`, which evaluates and copies its receiver where it is written, so a pointer receiver still writes through and a value one does not) and **method expressions** (`f := T.Area`, taking the receiver as the first parameter) — both usable through an interface |
| Pointers       | `&T{…}` / `&x` (a no-copy reference — go-rs composite values are heap handles), `*p` deref, `new(T)` (a pointer to a zero value of `T`); compound assignment through a pointer (`*p += n`, `*r ^= *r << 13`) at the pointee's width; an allocated pointer (`&T{…}`, `new(T)`) is **shared at every bind** — `q := p`, `f(p)`, a slice/array/map store, a `range` binding, a channel send, `append` — so writes through the second name are seen through the first, while `*p` and a value-receiver call still take a copy. `==` on an allocated pointer compares **identity** (two `errors.New("x")` are distinct) while struct values compare field by field. `&x` on an existing *variable* allocates a pointer object addressing that variable's storage, so it aliases through every bind, `*p = v` writes through, and two pointers to one variable are `==`; on a *scalar* there is no heap object to address, so `&x` is the value and `&a == &b` compares the values (BUGS.md). A pointer operand to a struct, array, slice or map prints as Go's does, `&{1 a}` / `&[1 2]`, and `%T` names it `*main.T` |
| Interfaces     | `type I interface{…}`; dynamic method dispatch on a value's runtime type; `any`/`interface{}` values, type assertions `x.(T)` (+ comma-ok `v, ok := x.(T)`), and type switches `switch v := x.(type) { case T: … }`. An interface **with a method set** — named or anonymous (`err.(interface{ Unwrap() error })`) — is matched by *method-set containment* on signatures, so `Unwrap() error` and `Unwrap() []error` are told apart, and embedded methods count. A struct value and a pointer to it are **distinct dynamic types** with Go's method sets — `case T` and `case *T` are told apart, a value has only its value-receiver methods (so `fmt` prints a value whose `String()` has a pointer receiver by its fields), and an embedded `*T` promotes `T`'s pointer methods too. `==` on an interface operand is decided by **dynamic type before value**, so `any(1) == any(1.0)` is false and an interface holding a nil slice is not `nil` |
| Closures       | function literals `func(…){…}` with **capture-by-reference** (a closure mutating a captured variable propagates, and closures share captured state); `f := func(){…}; f()`, IIFE, `go func(){…}()`; Go 1.22 per-iteration loop-variable capture. A closure that writes a package-level variable writes the one every function reads. A captured variable keeps its **declared type** inside the body, so a `uint8` still wraps at 8 bits, a `float32` still computes and prints at 32, a `uint64` still reads unsigned, and a captured channel is still a channel |
| First-class fns | `func(int) int` parameters and results — pass/return closures, higher-order fns (`apply`/`compose`/`reduce`); dynamic dispatch via the closure's stored subroutine id (`Op::CallDynamic`). A **declared** function is a value too (`apply(dbl)`, `f := dbl`, `[]func(int) int{dbl, inc}`, a func-typed struct field) — and so is a natively implemented stdlib function (`f := strings.ToUpper`, `apply(math.Sqrt, x)`, `pr := fmt.Println`, `conv := strconv.Atoi`) — it becomes a closure forwarding to it, because a dynamic call enters a subroutine whose slot 0 is the closure and a declared function's slot 0 is its first parameter. A function **type** keeps its whole signature (`func(string, ...int) (int, error)`), so a call through a func-typed parameter, field, element or result packs a variadic call's trailing arguments, destructures a multi-value result, and `%T` names the signature as Go does |
| Functions      | multiple parameters, variadic `func f(x ...int)` and variadic methods + spread `f(xs...)` (no trailing arguments bind a nil slice), `(T, U)` multi-value results, named results (`func f() (n int, err error)` — zero-initialized, bare `return`, deferred/`recover` mutation), `return a, b`, `x, y := f()` destructuring, multi-value spread `f(g())`, calling a function value from an index (`fns[i](x)`, `ops["k"](a, b)`, and through a container field: `r.handlers["k"](x)`) |
| Generics       | type parameters on funcs, types, and methods (`func F[T Number]`, `type Stack[T any]`, `Pair[K, V]{…}`), constraint interfaces (`~int \| ~float64`), inferred + explicit instantiation — **erased** onto the dynamic value model (no monomorphization) |
| defer          | `defer f(args)` — arguments snapshotted at defer time, deferred calls run LIFO on every return path; a deferred pointer-receiver method sees mutations made after the `defer` |
| panic / recover | `panic(v)` unwinds through defer drains, `recover()` stops it — with Go's frame rule: the panic is parked for the duration of each deferred call, so the deferred function runs normally (it may call other functions before recovering) and only a `recover()` it makes **itself** is effective; one from a function it called in turn returns nil. A deferred closure may set a named result on the panic path. **Runtime faults** (integer divide-by-zero, index-out-of-range, nil dereference) are recoverable too — `recover()` returns an `error` of the runtime's own type (`runtime.boundsError`, `runtime.errorString`, `runtime.plainError`, `*runtime.TypeAssertionError`) whose message is the `runtime error: …` text; an unrecovered panic prints `panic: <value>` and exits non-zero (matching Go, minus the goroutine trace) |
| Concurrency    | `go f(…)` goroutines (on any call — a declared function, a closure, a method `go w.run(ch)`, a func-typed parameter, field or element — with the callee and arguments evaluated at the `go` statement), `make(chan T[, cap])` (also through a defined channel type), the directional types `<-chan T` / `chan<- T` (accepted, direction not checked), `len(ch)` / `cap(ch)` (buffered count and capacity; `0` for a nil channel), `ch <- v` / `<-ch`, `close`, `for v := range ch` (receives until the channel is closed **and** drained), the comma-ok receive `v, ok := <-ch`, and `select` (with `default`, and the comma-ok case `case v, ok := <-ch:` that a closed channel makes ready) — buffered + unbuffered — on fusevm's cooperative scheduler; deadlocks are reported. `sync` (`WaitGroup`, `Mutex` + `TryLock`, `RWMutex`, `Once`) is vendored on top of it |
| Standard lib   | `fmt` (Println/Print/Printf + Sprintf/Sprint/Sprintln `%v %+v %#v %T %d %s %f %e %E %g %G %t %q %x %X %o %O %b %c %U %%` with width / `.precision` / `-` / `+` / `0` / `#` flags, floats rendered with strconv shortest-`g` semantics, and `Errorf` — builds a real `error` value, with `%w` recording the wrapped error(s) so `errors.Is`/`As`/`Unwrap` walk the chain; plus `Fprint`/`Fprintf`/`Fprintln`, which write to any `io.Writer` and yield its own `(n, err)`); `io` (`Writer`, `StringWriter`, `WriteString`, `ErrShortWrite`); `bufio` (`Writer` — `NewWriter`/`NewWriterSize`/`Write`/`WriteString`/`WriteByte`/`WriteRune`/`Flush`/`Available`/`Buffered`/`Size`/`Reset`, Go's own write half of `bufio.go`, so `w := bufio.NewWriter(os.Stdout); defer w.Flush()` buffers as Go does); `bytes` (`Buffer` — `Write`/`WriteString`/`WriteByte`/`WriteRune`/`String`/`Bytes`/`Len`/`Reset`, plus `NewBuffer`/`NewBufferString`; and the package functions `Equal`/`Compare`/`Index`/`IndexByte`/`IndexRune`/`IndexAny`/`IndexFunc`/`LastIndex`/`LastIndexByte`/`Contains`/`ContainsAny`/`ContainsRune`/`ContainsFunc`/`Count`/`HasPrefix`/`HasSuffix`/`Split`/`SplitN`/`SplitAfter`/`SplitAfterN`/`Fields`/`Join`/`Repeat`/`ToUpper`/`ToLower`/`ToTitle`/`Map`/`Trim`/`TrimLeft`/`TrimRight`/`TrimSpace`/`TrimPrefix`/`TrimSuffix`/`Runes`/`Replace`/`ReplaceAll`/`EqualFold`/`Cut`/`CutPrefix`/`CutSuffix`/`Clone`, Go's bodies over the native `strings` search and case mapping, returning subslices that alias the argument as Go's do); `strings` (ToUpper/ToLower/ToTitle — rune-for-rune over Go's own `unicode` case tables —/Contains/ContainsRune/ContainsAny/HasPrefix/HasSuffix/Trim/TrimLeft/TrimRight/TrimPrefix/TrimSuffix/TrimSpace/Split/SplitN/Fields/Join/Repeat/Index/IndexByte/IndexRune/IndexAny/LastIndex/LastIndexByte/Count/Compare/Replace/ReplaceAll/Title/EqualFold (simple case folding via `unicode.SimpleFold`), the `Map` / `*Func` family, `Cut`/`CutPrefix`/`CutSuffix`/`SplitAfter`/`SplitAfterN`/`LastIndexAny` and `NewReplacer` with `Replacer.Replace` (Go's own source, `goroot/strings_source.go`), plus `Builder` — `Write`/`WriteString`/`WriteByte`/`WriteRune`/`String`/`Len`/`Reset`/`Grow`, and a `*Builder` is an `io.Writer`); `strconv` (Itoa/FormatInt/FormatUint/FormatBool/FormatFloat — every format, the binary- and hex-exponent `b`/`x`/`X` included — Quote/QuoteRune/QuoteToASCII/QuoteRuneToASCII/QuoteToGraphic/QuoteRuneToGraphic/CanBackquote/IsPrint/IsGraphic over Go's own `IsPrint` range tables, and `IntSize`, `Unquote`/`UnquoteChar`/`QuotedPrefix` and the `Append*` family (`AppendInt`/`AppendQuote`/…) from Go's own `quote.go` (`goroot/strconv_source.go`), plus Atoi/ParseInt/ParseUint/ParseFloat/ParseBool — the integer parsers ported from Go's `atoi.go`, with base `0` prefixes (`0x`/`0o`/`0b`/`0`), `_` separators and `bitSize` ranges — returning Go's `(value, error)` pair — a real `*strconv.NumError` with its `Func`/`Num`/`Err` fields, wrapping the `ErrSyntax`/`ErrRange` sentinels, so `errors.Is`/`As`/`Unwrap` all work on it); `math` (Abs/Sqrt/Cbrt/Pow/Floor/Ceil/Round/Trunc/Mod/Hypot/Max/Min, trig Sin/Cos/Tan/Asin/Acos/Atan/Atan2/Sinh/Cosh/Tanh, Exp/Log/Log2/Log10, the IEEE specials Inf/NaN/IsInf/IsNaN/Signbit/Copysign and the bit patterns Float64bits/Float64frombits/Float32bits/Float32frombits, plus Modf/Frexp/Ldexp/Dim/Remainder/Log1p/Expm1/Gamma/Erf/Erfc/Nextafter/Nextafter32/RoundToEven compiled from Go's own math source (`goroot/math_source.go`) + consts Pi/E/Sqrt2/Phi/SqrtE/SqrtPi/SqrtPhi/Ln2/Log2E/Ln10/Log10E, MaxInt/MinInt and the sized MaxInt8..MaxInt64/MinInt8..MinInt64/MaxUint8..MaxUint64 and MaxUint, MaxFloat64/SmallestNonzeroFloat64/MaxFloat32/SmallestNonzeroFloat32); `sort` (Ints/Strings/Float64s, the searches, plus `Sort`/`Stable`/`IsSorted`/`Reverse`/`Slice`/`SliceStable`/`SliceIsSorted` and the `Interface` / `IntSlice` / `Float64Slice` / `StringSlice` types, compiled from Go's own `sort.go` + `zsortinterface.go` (pdqsort, SymMerge) so an unstable sort leaves equal elements in the order Go does); `os.Getenv`, `os.Args`, `os.Exit` (no deferred call runs) and `os.Stdout`/`os.Stderr` (a `*os.File` with `Write`/`WriteString`/`Fd`, so `fmt.Fprintln(os.Stderr, …)` is the usual diagnostic); builtins `len`/`cap`/`append`/`delete`/`make`/`new`/`close`/`clear`/`min`/`max`/`println`/`print` |
| Inline FFI     | `rust { pub extern "C" fn … }` blocks compile to a cached `cdylib` on first run and are callable by name from Go |

Goroutines, channels, and `select` run on a **cooperative scheduler in the
shared `fusevm` VM** (`fusevm::sched`, from the pinned `fusevm` 0.26.9): each
goroutine is its own VM sharing the program, the package-level variables and the single-threaded heap,
yielding at channel operations. **Generics are handled by erasure** — type-parameter and
type-argument brackets are consumed and dropped, and the dynamically-typed value
model runs one erased body for every instantiation (the zero value of a
type-parameter-typed `var` is nil, treated as the additive identity so a generic
accumulator matches Go for int/float/string). **Closures capture by reference**:
a variable captured by a nested closure is boxed in a shared heap cell, so a
closure's writes are seen by the enclosing scope and by sibling closures (loop
variables keep Go 1.22 per-iteration value semantics and are not boxed).
**`defer`/`panic`/`recover`** run
on host-side defer frames drained before every return — one per invocation,
addressed through a hidden local so interleaved goroutines each drain their
own: `defer` snapshots the
call's arguments (and, for a method, its receiver by reference) and pushes a
closure; a `panic` jumps to the function's defer drain and, if unrecovered,
propagates up the call chain (a compile-time check after each call, active only
in programs that panic). Documented simplifications: method receivers use
reference semantics (a value receiver is not copied), and an unrecovered panic
prints its message but not Go's goroutine stack trace.
**Field promotion is resolved at run time** by treating a field whose name equals
its value's type name as embedded — which is exactly how the parser records
`struct { Base }`, but also matches a hand-written `Base Base` field, so that
field would promote here where Go would reject the reference as undefined.
Variables are block-scoped as in Go: a declaration that reuses an outer
variable's name in an inner block (`for j := …` inside `for j := …`, an
`if err := …` init, a `switch v := v.(type)` binding) is a new variable —
`scope.rs` renames it apart before the name-keyed compiler sees it.
A `type` declaration inside a function body is parsed but **hoisted** to the
package rather than scoped to its block, so two blocks declaring different types
under one name collide (the first parsed wins).

## Toolchain

The full editor/tooling surface ships in the one `go` binary, at parity with the
other `fusevm` frontends:

- **LSP** (`go --lsp`) — completion, hover, and parser-driven diagnostics over stdio.
- **DAP** (`go --dap`) — line breakpoints, stepping, stack trace, and locals inspection.
- **zsh completion** — `completions/_go`.
- **man pages** — `man/man1/go.1` and `man/man1/goall.1`.
- **HTML docs** — [`docs/`](docs) (index, engineering report, and a `reference.html`
  generated from the LSP corpus by the `gen-docs` binary).
- **Inline Rust FFI** — `rust {}` blocks via the shared `fusevm` FFI runtime.
- **Introspection** — `--dump-tokens` / `--dump-ast` / `--disasm` / `--tiers`.

## Build & test

```sh
cargo build
cargo test
```

CI enforces `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
and `cargo doc` with `-D warnings`.

## Differential parity vs the reference `go`

Two dev harnesses check go-rs output **byte-for-byte against the real `go`
toolchain** (needs `go` on `PATH`; not run in CI):

```sh
# 1. curated corpus of idiomatic programs
bash parity-scripts/run.sh          # BYTE PARITY: N / N match

# 2. grammar-driven fuzzer — thousands of deterministic-output snippets
cargo run --bin parity-fuzz -- --count 2000
cargo run --bin parity-fuzz -- --seed 1234 --once   # replay one divergence
```

The corpus covers arithmetic, control flow, recursion, `Printf` format specs,
slices/maps (including the typed nil a slice or map zero value is), structs/
methods, interfaces and interface conversions, multi-name `var` declarations,
`float32` width, `strconv`'s `*NumError`, closures, generics, goroutines/channels,
`select`, channel `range`/comma-ok receive, `recover()`'s frame rules, unsigned
64-bit integers, labelled loop signals, and the declared type a captured
variable keeps inside a closure (a `uint8` still wraps, a `float32` still
rounds to 32 bits, a captured channel is still a channel). It also covers the
parts of `fmt` a malformed or unusual call reaches — a missing or extra operand,
a `%` that never reaches a verb, an unknown verb, `*` width and precision, the
explicit `%[n]` operand index and the `%!verb(BADINDEX)` forms it rejects — plus
the space flag, the `0` flag on the non-numeric verbs (`%010q`, `%010T`) and its
one exception (`%U`), a width on `%v` landing on each *element* of a composite,
the minimum-digit-count precision an *integer* verb takes (as
against the truncation a string takes), `%T` of every sized integer width, a
`f(args...)` spread into a `fmt` call, string/rune iteration against byte
indexing, and `continue` in every loop form. Further files cover map key
equality and the order a map keeps through inserts and deletes, `error` as a
method set (a type switch and an assertion against it, and the conversion panic
each raises), the comma-ok forms assigning into existing variables, `type`
declared inside a function body and the grouped `type ( … )` form, which slice
operations share a backing array and which reallocate, a non-ASCII rune literal
next to punctuation, the added `strings`/`strconv` functions with their
edge cases, and `strconv.FormatFloat` over every verb x precision x bit-size
combination. The fuzzer generates arithmetic /
float / boolean / string / slice /
map / control-flow / stdlib blocks plus rune arithmetic, fixed-size arrays
(sequential + sparse), `[]byte`/`[]rune` conversions, string-range-by-rune,
three-index slices, structs with value/pointer-receiver methods, `new(T)`,
`fmt.Errorf`/`errors.New`, `defer`/`recover` on runtime panics, type switches,
capturing closures, bitwise operators, shortest-representation float output
(`%v`/`%g`/`%e`, where exponent notation appears), integer division through a
slice element, and generic instantiation. Five further shapes cover unsigned
64-bit integers, the narrow fixed widths (`int8`…`uint32`: wrapping, arithmetic
vs logical `>>`, shifts at or past the width, conversion truncation), labelled
and unlabelled `break`/`continue` nested two deep plus `switch` fallthrough,
`defer`/`panic`/`recover` frame rules, and channels (`range`, comma-ok receive,
`select` with `default`). A further shape covers **struct value semantics**
through a nested struct: copy on assignment, argument bind, return, slice and
map store, indexed read, `range` binding, `append` (including a spread),
channel send, value- vs pointer-receiver calls, and field-wise `==`. Another
covers **array value semantics** over the same sites, plus the depths a struct
does not reach: nested `[N][M]T`, an array of structs, an array-typed struct
field, `range` over an array written mid-loop, an array map key, and the
reference half (an array of slices keeps sharing its slices). A further shape
covers the **fixed-size array's type name**, which rides on the value rather
than the static type: `%T`/`%#v`/`%v` on an array beside the slice spelling that
must still name a slice, through an assignment, an `any` box, a nested
`[2][3]int`, an array of structs, an array of slices, a slice and a map *of*
arrays, and the `float32`/`uint64` widths whose `fmt` boxing rebuilds the value.
Another covers **composite literals past fusevm's 255-value call arity**,
which are built in chunks: a slice, an array, a map and a variadic spread all
sized over the cut, checking an element *past* the cut rather than only the
length — the old wrap-around silently produced a short literal. A last one
covers **interface equality**, which Go decides by dynamic type before value:
the same number held as an `int`, a `float64` and its own text, a `bool` beside
the string spelling of one, an untyped `nil`, and a nil slice or map compared
both directly (true) and through an interface (false). The matched pairs are
printed alongside the mismatched ones, so neither a blanket `true` nor a blanket
`false` passes. The fixed-width shape runs its arithmetic both directly and
inside a capturing closure, which are separate code paths. It diffs both
interpreters byte-for-byte (stdout + exit status).

`--only N` pins every generated block to statement shape `N`, so one shape's
divergence rate is measurable instead of diluted across the other 38, and
`--ours PATH` runs a go-rs binary built from another commit — together they are
how a newly added shape is shown to actually exercise what it claims to.

A case only counts as a comparison when the **reference itself succeeded**:
exit 0 with something on stdout. Go rejects an unused import or an unused
variable at compile time, so a generator slip yields a program `go` never runs
— and since go-rs would usually reject it too, "neither printed anything and
both failed" would otherwise score as agreement. Those cases are reported as
`skipped` and excluded from the rate, because two failures agreeing is not a
comparison and a mode that mostly generates them is measuring nothing.

Differences that are known and still open are written down in
[`BUGS.md`](BUGS.md) with a reproducer and what closing each one needs. The
corpus is a green gate, so a gap lives in `BUGS.md` until the fix and its corpus
file land together.

**Packages are run from source.** An `import` of a non-native package is resolved
to its Go source, parsed, name-qualified (`errors.New` → the linked `errors.New`),
and compiled into the same unit as `main` (see [`src/pkg.rs`](src/pkg.rs)) — the standard
library is *executed*, not reimplemented. A small native layer stays as host
builtins for the irreducible runtime/I-O boundary (`fmt` writes stdout, `os`
touches the OS). The vendored stdlib ([`goroot/`](goroot)) grows as go-rs gains the
language features each package needs; a not-yet-supported import is a clear error.

Constant float expressions are **folded exactly** — go-rs evaluates a
compile-time-constant float expression (`1.950 * 10.187`, `0.1 + 0.2`) with exact
rational arithmetic and rounds to `f64` once, matching Go's arbitrary-precision
constant semantics (a very long decimal or a non-terminating division whose exact
terms leave the `f64`-exact range falls back to runtime `f64`).

**Bundled packages.** The standard-library packages vendored into the binary
(`errors`, `sync`, `unicode/utf16`, `cmp`, `io`, `bufio` (its `Writer`), `bytes`,
`iter`, `slices`, `maps`, `math/bits`) always resolve to the binary's own copy; any other import resolves from `~/.go-rs/src`, then from
`$GOROOT/src`. `go install-std` writes the vendored packages into
`~/.go-rs/src` as readable source. Any package placed under `~/.go-rs/src`
(or `$GOPATH/src`) is importable — go-rs is an executor for real Go source, not a
curated subset. A package's defined non-struct types (`type SpecialCase
[]CaseRange`) are qualified and linked like its structs, which is what lets
`unicode` run from its real source, and `%T` names a package's type by its
package (`week.Day`, not `main.Day`).

`strings.Map` and the `strings` functions taking a function (`IndexFunc`,
`LastIndexFunc`, `ContainsFunc`, `TrimFunc`, `TrimLeftFunc`, `TrimRightFunc`,
`FieldsFunc`) are written in Go ([`goroot/strings_func.go`](goroot/strings_func.go))
and synthesized into a program that imports `strings`, because a native builtin
cannot call the closure it is handed.

**Blockers** (defects to close, not intentional scope — go-rs targets a Go
superset):

- **Dependencies on the compiler/runtime boundary.** A package that reaches
  `unsafe`, `//go:linkname` to runtime symbols, `.s` assembly, `cgo`, or
  `reflect` cannot yet run from source. Four such packages are vendored with
  that edge replaced: `math/bits` with the
  `compiler_bootstrap` error file; `iter` without its runtime coroutines
  (`Pull` / `Pull2` run the sequence on a goroutine); `slices` with
  `overlaps` asking the host whether two slices share a backing array instead
  of comparing `unsafe` addresses; `maps` with `Clone` copying in Go instead
  of calling the runtime's `clone`.
  `fmt`/`strings`/`strconv`/`math`/`sort`/`os` are provided by the native
  runtime layer instead.
- **Generics are erased, so a type-parameter zero value is untyped** — `var zero
  T; x != zero` (e.g. `cmp.Or`) compares against `nil` rather than the
  instantiated type's zero. Needs monomorphization or a typed-zero sentinel.
- **`uint64` loses its signedness through an `any` parameter.** `uint64`, `uint`
  and `uintptr` are correct on their own: `/`, `%`, `>>`, the ordered
  comparisons and the conversion to a float are done unsigned, and `fmt` prints
  the unsigned digits for `%d`/`%v`/`%x`/`%o`/`%b` — including through slice
  elements, map values and struct fields. As with `float32`, the width is read
  from the static type at the `fmt` call site, so a value that has passed
  through an `any`/interface parameter prints as its signed `i64`. The narrower
  widths — `int8`/`int16`/`int32`, `uint8`/`uint16`/`uint32`, `byte`, `rune` —
  wrap at their declared width through `++`, compound assignment, binary and
  unary operators, struct fields, slice elements and function results.
- **Two interfaces holding two integer *widths* compare equal.** Interface `==`
  is decided by dynamic type before value, so `any(1) == any(1.0)` is false, so
  is `any(1) == any("1")`, so are two struct types with the same field, and an
  interface holding a nil slice is not equal to `nil`. Only the integer widths
  are left: `int`, `int64`, `uint`, `byte` and `rune` are all one 64-bit value
  and all name `int`, so `any(97) == any(byte(97))` is true where Go says false.
  It wants the same value-side type tag `float32` and `uint64` do
  (see [`BUGS.md`](BUGS.md)).
- **`float32` loses its width through an `any` parameter or a nested struct
  field.** Arithmetic runs at 32-bit width and `fmt` prints the 32-bit shortest
  decimal for a statically-`float32` operand (including `[]float32`,
  `map[K]float32` and the operand struct's own fields), but the width is read
  from the static type at the `fmt` call site, so it does not survive erasure.
- **A defined type reached through a type parameter or inside a struct field or
  map value prints bare.** A defined value converted to an interface carries its
  type name, so methods dispatch through the interface, type switches and
  assertions match it, `==` compares type then value, and `fmt` prints it — and
  a `[]T` of it, a method result of its type, and an element of a defined slice
  of it — through `String()` / `Error()`. A generic parameter is erased
  to its constraint and a nested field is formatted by the host, so
  `show[T any](Weekday(3))` prints `3` where Go prints the method's text (see
  [`BUGS.md`](BUGS.md)).
- **`%T` of an empty map describes it from its contents**, so `map[string]int{}`
  prints as `map[interface {}]interface {}`. A map whose written type mentions a
  defined type is named exactly even when empty; a slice always is.
- **An unassigned code point prints literally where Go escapes it**, so `%q` of
  `0x378` is the raw rune rather than its `͸` escape. `unicode.IsPrint`'s
  other three non-printable classes (the controls, the separators and the
  private-use areas) are decided exactly; `Cn` needs the Unicode general-category
  tables (see [`BUGS.md`](BUGS.md)).
- **A call passes at most 255 arguments.** fusevm carries a call's argument
  count in a `u8`, and unlike a composite literal a call site has nothing to
  build up in chunks, so `fmt.Println` with 256 arguments is a compile error.
  Go itself has no such limit. A composite literal is *not* bounded this way —
  it is built in chunks at any size.

## License

MIT.
