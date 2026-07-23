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
go version            # print the version banner
go --dump-tokens f.go # lexer token stream (with inserted semicolons)
go --dump-ast f.go    # parsed AST
go --disasm f.go      # lowered fusevm bytecode
go --lsp              # Language Server Protocol over stdio
go --dap              # Debug Adapter Protocol over stdio
```

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

This is slice 1 — a single-file `package main` that runs real Go programs:

| Area          | Supported                                                              |
| ------------- | --------------------------------------------------------------------- |
| Declarations  | `package`, `import` (single + grouped), top-level `func` with params/results |
| Variables     | `:=`, `var x [T] [= e]`, assignment, `+= -= *= /= %=`, `x++` / `x--`   |
| Control flow  | `if` / `else if` / `else` (with init clause), three-clause / condition / infinite `for`, `break`, `continue`, `return` |
| Expressions   | int / float / string / bool literals, arithmetic, comparisons, `&&` `\|\|` `!` (short-circuit), unary, parentheses, calls, recursion |
| Types         | `int` family, `float32/64`, `string`, `bool` — tracked statically so `int / int` truncates and `float / float` stays exact |
| Printing      | `fmt.Println`, `fmt.Print`, `fmt.Printf` (`%v %d %s %f %t %q %%`), builtin `println` / `print` |
| Inline FFI    | `rust { pub extern "C" fn … }` blocks compile to a cached `cdylib` on first run and are callable by name from Go |

Structs, methods, interfaces, slices, maps, channels, and goroutines land in
later waves.

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

## License

MIT.
