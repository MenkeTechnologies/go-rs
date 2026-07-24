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
ordering are dispatched through fusevm's strict numeric hook.

## Pipeline

```
Go source
   │  lexer.rs      — tokens + automatic semicolon insertion (ASI)
   ▼
tokens
   │  parser.rs     — recursive-descent → Go AST
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
go version            # print the version banner
go help [command]     # usage (optionally for one command)
go --dump-tokens f.go # lexer token stream (with inserted semicolons)
go --dump-ast f.go    # parsed AST
go --disasm f.go      # lowered fusevm bytecode
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
| Declarations   | `package`, `import` (single + grouped), `type T struct`, top-level `func` and methods (`func (r T) m()`) |
| Variables      | `:=`, `var x [T] [= e]`, assignment to lvalues (ident / `x[i]` / `x.f`), parallel assignment `a, b = x, y` (swap/rotate; RHS evaluated first), `a, b = f()`, `+= -= *= /= %=`, `x++` / `x--` |
| Control flow   | `if` / `else if` / `else` (with init clause), three-clause / condition / infinite `for`, `for … range`, `switch` (tagged / expression / multi-value cases / init clause / `fallthrough`) and type switch, `break`, `continue`, `return` |
| Expressions    | int / float / string / bool literals (incl. `0x` / `0o` / `0b` bases, `_` separators, and uint64 masks above `i64::MAX` stored by bit pattern), rune literals as int32 code points (`'A'` == 65, `'z' - '0'`) with the full escape set (`\n \t \xHH \uHHHH \UHHHHHHHH` + octal, in rune **and** string literals), arithmetic, bitwise `& \| ^ << >> &^` (+ `^x` complement, compound `&= \|= ^= <<= >>= &^=`), comparisons, `&&` `\|\|` `!` (short-circuit), unary, parentheses, calls, recursion |
| Types          | `int` family, `float32/64`, `string`, `bool`, defined types (`type Celsius float64`) — tracked statically so `int / int` truncates and `float / float` stays exact; conversions `T(x)` (`int(f)`, `float64(n)`, `string(rune)`, `byte`/`rune`/…) and slice conversions `[]byte(s)` / `[]rune(s)` (and `string([]byte)` / `string([]rune)` back) |
| Constants      | `const x = …` and grouped `const ( … )` blocks with `iota` (auto-increment, expression repetition, `1 << iota` flag patterns) |
| Slices         | `[]T{…}`, `make([]T, n)`, `s[i]`, `s[i] = v`, slice expressions `s[lo:hi]` / `s[:hi]` / `s[lo:]` / three-index `s[lo:hi:max]` (capacity bound accepted; also on strings) that **share the backing array** (writes alias the parent; `cap` reflects the offset; `append` writes in place when the backing has room, else reallocates), `len` / `cap` / `append`, `for i, v := range s`; ranging a **string** yields runes (byte offset + code point, once per rune) |
| Arrays         | fixed-size `[N]T` / `[...]T` (modeled as slices): sequential `[3]int{…}`, sparse index-keyed `[N]T{3: v}` with zero-fill, struct elements with elided `{…}`, and bare `var buf [N]scalar` zero-filled to N |
| Maps           | `map[K]V{…}`, `make(map[K]V)`, `m[k]`, `m[k] = v`, `delete`, `len`, `for k, v := range m` |
| Structs        | `type T struct{…}`, literals `T{…}` / `T{f: v}`, field read/write `s.f`, **value-copy semantics** on assign/pass/return |
| Methods        | value/pointer receivers, `recv.m(args)` dispatch by receiver type |
| Pointers       | `&T{…}` / `&x` (a no-copy reference — go-rs composite values are heap handles), `*p` deref, `new(T)` (a pointer to a zero value of `T`); a pointer shares the pointed-to struct |
| Interfaces     | `type I interface{…}`; dynamic method dispatch on a value's runtime type; `any`/`interface{}` values, type assertions `x.(T)` (+ comma-ok `v, ok := x.(T)`), and type switches `switch v := x.(type) { case T: … }` |
| Closures       | function literals `func(…){…}` with **capture-by-reference** (a closure mutating a captured variable propagates, and closures share captured state); `f := func(){…}; f()`, IIFE, `go func(){…}()`; Go 1.22 per-iteration loop-variable capture |
| First-class fns | `func(int) int` parameters and results — pass/return closures, higher-order fns (`apply`/`compose`/`reduce`); dynamic dispatch via the closure's stored subroutine id (`Op::CallDynamic`) |
| Functions      | multiple parameters, variadic `func f(x ...int)` + spread `f(xs...)`, `(T, U)` multi-value results, named results (`func f() (n int, err error)` — zero-initialized, bare `return`, deferred/`recover` mutation), `return a, b`, `x, y := f()` destructuring, multi-value spread `f(g())`, calling a function value from an index (`fns[i](x)`, `ops["k"](a, b)`) |
| Generics       | type parameters on funcs, types, and methods (`func F[T Number]`, `type Stack[T any]`, `Pair[K, V]{…}`), constraint interfaces (`~int \| ~float64`), inferred + explicit instantiation — **erased** onto the dynamic value model (no monomorphization) |
| defer          | `defer f(args)` — arguments snapshotted at defer time, deferred calls run LIFO on every return path; a deferred pointer-receiver method sees mutations made after the `defer` |
| panic / recover | `panic(v)` unwinds through defer drains, `recover()` (in a deferred closure) stops it; **runtime faults** (integer divide-by-zero, index-out-of-range, nil dereference) are recoverable too — `recover()` returns the `runtime error: …` value; an unrecovered panic prints `panic: <value>` and exits non-zero (matching Go, minus the goroutine trace) |
| Concurrency    | `go f(…)` goroutines, `make(chan T[, cap])`, `ch <- v` / `<-ch`, `close`, `select` (with `default`) — buffered + unbuffered — on fusevm's cooperative scheduler; deadlocks are reported |
| Standard lib   | `fmt` (Println/Print/Printf + Sprintf/Sprint/Sprintln `%v %d %s %f %t %q %%`, and `Errorf` — builds a real `error` value, `%w` formats like `%v`); `strings` (ToUpper/ToLower/Contains/HasPrefix/HasSuffix/Trim/TrimPrefix/TrimSuffix/TrimSpace/Split/Fields/Join/Repeat/Index/LastIndex/Count/ReplaceAll/Title/EqualFold); `strconv` (Itoa/Atoi/ParseInt/ParseFloat/FormatInt/Quote); `math` (Abs/Sqrt/Pow/Floor/Ceil/Round/Trunc/Mod/Hypot/Max/Min + Pi/E); `sort` (Ints/Strings/Float64s); `os.Getenv`; builtins `len`/`cap`/`append`/`delete`/`make`/`new`/`close`/`min`/`max`/`println`/`print` |
| Inline FFI     | `rust { pub extern "C" fn … }` blocks compile to a cached `cdylib` on first run and are callable by name from Go |

Goroutines, channels, and `select` run on a **cooperative scheduler added to the
shared `fusevm` VM** (`fusevm::sched`, v0.14.14–0.14.15): each goroutine is its
own VM sharing the program and the single-threaded heap, yielding at channel
operations. **Generics are handled by erasure** — type-parameter and
type-argument brackets are consumed and dropped, and the dynamically-typed value
model runs one erased body for every instantiation (the zero value of a
type-parameter-typed `var` is nil, treated as the additive identity so a generic
accumulator matches Go for int/float/string). **Closures capture by reference**:
a variable captured by a nested closure is boxed in a shared heap cell, so a
closure's writes are seen by the enclosing scope and by sibling closures (loop
variables keep Go 1.22 per-iteration value semantics and are not boxed).
**`defer`/`panic`/`recover`** run
on a host-side defer stack drained before every return: `defer` snapshots the
call's arguments (and, for a method, its receiver by reference) and pushes a
closure; a `panic` jumps to the function's defer drain and, if unrecovered,
propagates up the call chain (a compile-time check after each call, active only
in programs that panic). Documented simplifications: a map read of a missing key
returns the numeric zero (`0`) rather than the comma-ok form; method receivers
use reference semantics (a value receiver is not copied); `go` targets a
top-level function call (no closure goroutines yet); a deferred closure that
mutates a **named** return value does not propagate that change (pending
capture-by-reference), and an unrecovered panic prints its message but not Go's
goroutine stack trace.

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
- **Introspection** — `--dump-tokens` / `--dump-ast` / `--disasm`.

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
slices/maps, structs/methods, interfaces, closures, generics, goroutines/channels,
and `select`. The fuzzer generates arithmetic / float / boolean / string / slice /
map / control-flow / stdlib blocks plus rune arithmetic, fixed-size arrays
(sequential + sparse), `[]byte`/`[]rune` conversions, string-range-by-rune,
three-index slices, structs with value/pointer-receiver methods, `new(T)`,
`fmt.Errorf`/`errors.New`, `defer`/`recover` on runtime panics, type switches,
capturing closures, bitwise operators, and generic instantiation — and diffs both
interpreters byte-for-byte (stdout + exit status).

**Packages are run from source.** An `import` of a non-native package is resolved
to its Go source, parsed, name-qualified (`errors.New` → the linked `errors.New`),
and compiled into the same unit as `main` (see [`src/pkg.rs`]) — the standard
library is *executed*, not reimplemented. A small native layer stays as host
builtins for the irreducible runtime/I-O boundary (`fmt` writes stdout, `os`
touches the OS). The vendored stdlib ([`goroot/`]) grows as go-rs gains the
language features each package needs; a not-yet-supported import is a clear error.

Constant float expressions are **folded exactly** — go-rs evaluates a
compile-time-constant float expression (`1.950 * 10.187`, `0.1 + 0.2`) with exact
rational arithmetic and rounds to `f64` once, matching Go's arbitrary-precision
constant semantics (a very long decimal or a non-terminating division whose exact
terms leave the `f64`-exact range falls back to runtime `f64`).

**Known gaps** (documented rather than hidden):

- **Standard-library coverage is incremental.** Packages run from their real Go
  source, but a package only works once go-rs supports every language feature it
  (transitively) uses — packages needing `unsafe`, assembly, `cgo`, `reflect`, or
  runtime internals don't load yet. `fmt`/`strings`/`strconv`/`math`/`sort`/`os`
  are provided by the native runtime layer.
- **No fixed-width integer overflow wrapping** — all integers are 64-bit, so code
  relying on `uint8`/`uint16`/`uint32` wraparound (some hashes) diverges. A uint64
  constant prints as its signed `i64` (its bit pattern is correct for bitwise use).
- **Struct value semantics have a replication boundary** — a bare `var a [N]Struct`
  (no literal) and circular self-referential structures share one element handle
  rather than N distinct zero values; array/slice literals with explicit struct
  elements copy correctly.

## License

MIT.
