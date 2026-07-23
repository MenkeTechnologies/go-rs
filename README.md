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
There is no module system, so `go get` / `go mod` are out of scope.

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

A single-file `package main` that runs real Go programs:

| Area           | Supported                                                              |
| -------------- | --------------------------------------------------------------------- |
| Declarations   | `package`, `import` (single + grouped), `type T struct`, top-level `func` and methods (`func (r T) m()`) |
| Variables      | `:=`, `var x [T] [= e]`, assignment to lvalues (ident / `x[i]` / `x.f`), `+= -= *= /= %=`, `x++` / `x--` |
| Control flow   | `if` / `else if` / `else` (with init clause), three-clause / condition / infinite `for`, `for … range`, `break`, `continue`, `return` |
| Expressions    | int / float / string / bool literals, arithmetic, comparisons, `&&` `\|\|` `!` (short-circuit), unary, parentheses, calls, recursion |
| Types          | `int` family, `float32/64`, `string`, `bool` — tracked statically so `int / int` truncates and `float / float` stays exact |
| Slices         | `[]T{…}`, `make([]T, n)`, `s[i]`, `s[i] = v`, `len` / `cap` / `append`, `for i, v := range s` |
| Maps           | `map[K]V{…}`, `make(map[K]V)`, `m[k]`, `m[k] = v`, `delete`, `len`, `for k, v := range m` |
| Structs        | `type T struct{…}`, literals `T{…}` / `T{f: v}`, field read/write `s.f`, **value-copy semantics** on assign/pass/return |
| Methods        | value/pointer receivers, `recv.m(args)` dispatch by receiver type |
| Interfaces     | `type I interface{…}`; dynamic method dispatch on a value's runtime type (a compiled type-switch over the concrete implementors) |
| Closures       | function literals `func(…){…}` with capture-by-value; `f := func(){…}; f()`, IIFE, `go func(){…}()` |
| First-class fns | `func(int) int` parameters and results — pass/return closures, higher-order fns (`apply`/`compose`/`reduce`); dynamic dispatch via the closure's stored subroutine id (`Op::CallDynamic`) |
| Functions      | multiple parameters, `(T, U)` multi-value results, `return a, b`, and `x, y := f()` destructuring |
| Concurrency    | `go f(…)` goroutines, `make(chan T[, cap])`, `ch <- v` / `<-ch`, `close`, `select` (with `default`) — buffered + unbuffered — on fusevm's cooperative scheduler; deadlocks are reported |
| Standard lib   | `fmt` (Println/Print/Printf `%v %d %s %f %t %q %%`); `strings` (ToUpper/ToLower/Contains/HasPrefix/HasSuffix/Trim/TrimPrefix/TrimSuffix/TrimSpace/Split/Fields/Join/Repeat/Index/LastIndex/Count/ReplaceAll/Title/EqualFold); `strconv` (Itoa/Atoi/ParseInt/ParseFloat/FormatInt/Quote); `math` (Abs/Sqrt/Pow/Floor/Ceil/Round/Trunc/Mod/Hypot/Max/Min + Pi/E); `sort` (Ints/Strings/Float64s); `os.Getenv`; builtins `len`/`cap`/`append`/`delete`/`make`/`close`/`min`/`max`/`println`/`print` |
| Inline FFI     | `rust { pub extern "C" fn … }` blocks compile to a cached `cdylib` on first run and are callable by name from Go |

Goroutines, channels, and `select` run on a **cooperative scheduler added to the
shared `fusevm` VM** (`fusevm::sched`, v0.14.14–0.14.15): each goroutine is its
own VM sharing the program and the single-threaded heap, yielding at channel
operations. `defer` and generics are future waves. Documented
simplifications: a map read of a missing key returns the numeric zero (`0`)
rather than the comma-ok form; method receivers use reference semantics (a value
receiver is not copied); and `go` targets a top-level function call (no closure
goroutines yet).

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
slices/maps, structs/methods, interfaces, closures, goroutines/channels, and
`select`. The fuzzer generates arithmetic / float / boolean / string / slice /
map / control-flow / stdlib blocks and diffs both interpreters. go-rs runs
single-file `package main` against its built-in stdlib subset — it has no module
system, so `go get` / third-party imports are out of scope.

**Known, characterized divergences** (the harness surfaced these; they are
documented rather than hidden):

- **Constant folding of float arithmetic.** Go evaluates a *constant* float
  expression (`1.950 * 10.187`) with arbitrary precision and rounds once; go-rs
  evaluates it as runtime `f64` (double-rounded, like Rust), so the last digit of
  a `%f` can differ. go-rs matches Go's *variable/runtime* float semantics
  (`a := 1.950; a * b`) exactly — the fuzzer routes float operands through
  variables to test that.
- **Closures capture by value, not by reference** — a closure that *mutates* a
  captured variable does not propagate the change.

## License

MIT.
