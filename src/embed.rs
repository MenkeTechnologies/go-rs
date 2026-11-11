//! Run a Go program from Rust with a given stdin and get its stdout back.

use crate::stdio;

/// Stack for the interpreter thread: the 8 MiB a process main thread gets,
/// which is what the `go` binary parses, lowers and runs on.
const STACK_BYTES: usize = 8 * 1024 * 1024;

/// Run `src` as a complete program with `input` as its stdin and return
/// everything it wrote to stdout.
///
/// Each call runs on a new thread, so the object heap and every other
/// thread-local runtime table start empty and concurrent calls share nothing.
/// Reads of `os.Stdin` (`bufio.Scanner`, `bufio.Reader`, `fmt.Scan`,
/// `io.ReadAll`) see `input`; `fmt.Print*` and `os.Stdout` writes from every
/// goroutine are collected instead of reaching the process's stdout (stderr
/// writes, including the `println`/`print` builtins, still go to stderr). A
/// parse or compile error, an unrecovered panic, a deadlock, `os.Exit` with a
/// nonzero status or a panic in the runtime returns `Err`; `os.Exit(0)` returns
/// `Ok` with the output written before it. The process is never exited.
///
/// When a goroutine other than `main` exits or panics, the remaining goroutines
/// run on until they block or finish (their output is discarded), since the
/// scheduler has no way to stop them early.
pub fn run_program(src: &str, input: &str) -> Result<String, String> {
    let (src, input) = (src.to_string(), input.to_string());
    let worker = std::thread::Builder::new()
        .name("gors".to_string())
        .stack_size(STACK_BYTES)
        .spawn(move || {
            stdio::begin(&input);
            let outcome = crate::run_str(&src);
            let (out, exit) = stdio::end();
            match (exit, outcome) {
                (Some((0, _)), _) | (None, Ok(_)) => Ok(out),
                (Some((status, diagnostic)), _) if diagnostic.is_empty() => {
                    Err(format!("exit status {status}"))
                }
                (Some((_, diagnostic)), _) => Err(diagnostic.trim_end().to_string()),
                (None, Err(e)) => Err(e),
            }
        })
        .map_err(|e| format!("go-rs: cannot start interpreter thread: {e}"))?;
    worker.join().unwrap_or_else(|payload| {
        let msg = payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "interpreter panicked".to_string());
        Err(format!("go-rs: panic: {msg}"))
    })
}
