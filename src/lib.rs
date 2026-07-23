//! go-rs — Go as a fusevm frontend.
//!
//! Pipeline: `lexer` (with Go automatic semicolon insertion) → `parser` builds a
//! Go AST → `compiler` lowers it to a `fusevm::Chunk` → fusevm executes it on
//! the shared three-tier Cranelift JIT, calling back into `host` (the strict
//! numeric hook) for Go's `+` string-concatenation overload and string
//! ordering. There is no bespoke VM and no Go runtime here — execution and
//! codegen live in fusevm, the same engine behind zshrs, strykelang, awkrs,
//! vimlrs, elisprs, rubylang, javars, kotlinrs, and scalars.

pub mod ast;
pub mod banner;
pub mod cli;
pub mod compiler;
pub mod dap;
pub mod host;
pub mod lexer;
pub mod lsp;
pub mod parser;
pub mod rust_ffi;

pub use banner::version_banner;
use fusevm::{VMResult, Value, VM};

/// Parse Go `src` to an AST. Inline `rust { ... }` FFI blocks are desugared to
/// `__rust_compile(...)` statements first (see [`rust_ffi`]), so every parse
/// path — run, `--dump-ast`, `--disasm`, `--dap` — sees the same rewritten
/// source. No-op when the source has no `rust` block.
pub fn parse(src: &str) -> Result<ast::Program, String> {
    let src = rust_ffi::desugar(src);
    parser::parse(&src)
}

/// Parse and lower Go `src` to a runnable fusevm chunk.
pub fn compile(src: &str) -> Result<fusevm::Chunk, String> {
    let prog = parse(src)?;
    compiler::compile(&prog)
}

/// Register the go-rs builtins + strict numeric hook on a fresh VM, enable the
/// tracing JIT, and run the chunk. Returns the last top-of-stack value.
fn run_chunk(chunk: fusevm::Chunk) -> Result<Value, String> {
    let mut vm = VM::new(chunk);
    host::install(&mut vm);
    vm.set_numeric_hook(std::sync::Arc::new(host::numeric_hook));
    vm.enable_tracing_jit();
    let result = vm.run();
    // An inline-Rust FFI fault stashes its message and halts the VM; it must be
    // checked on both the `Halted` and `Ok` paths (a fault mid-expression can
    // leave a value on the stack), so surface it before reporting success.
    if let Some(e) = host::take_ffi_error() {
        return Err(e);
    }
    match result {
        VMResult::Ok(v) => Ok(v),
        VMResult::Halted => Ok(vm.stack.last().cloned().unwrap_or(Value::Undef)),
        VMResult::Error(e) => Err(e),
    }
}

/// Compile and run a Go source string; return the last VM value.
pub fn run_str(src: &str) -> Result<Value, String> {
    let chunk = compile(src)?;
    run_chunk(chunk)
}

/// Read and run a `.go` file.
pub fn run_file(path: &str) -> Result<Value, String> {
    let src =
        std::fs::read_to_string(path).map_err(|e| format!("go-rs: cannot read {path}: {e}"))?;
    run_str(&src)
}

/// Compile `src` and return a human-readable disassembly of the fusevm chunk
/// (for `go --disasm`).
pub fn disassemble(src: &str) -> Result<String, String> {
    Ok(compile(src)?.disassemble())
}
