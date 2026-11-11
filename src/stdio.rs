//! The program's stdin/stdout and process exit, as the host builtins reach them.
//!
//! By default these are the process's own: writes go to fd 1, reads come from
//! fd 0, and `os.Exit` / a fatal panic / a deadlock end the process. While an
//! embedder ([`crate::embed::run_program`]) has a run open on this thread, the
//! same calls read the embedder's input, collect output in a per-thread buffer,
//! and turn an exit into a recorded status plus a VM halt.

use std::cell::RefCell;
use std::io::{Read, Write};

/// One embedded run's streams.
struct Embedded {
    input: Vec<u8>,
    read_pos: usize,
    out: Vec<u8>,
    /// The status the program exited with, and the diagnostic (a panic's
    /// `panic: ...` lines) that went with it.
    exit: Option<(i32, String)>,
}

thread_local! {
    static EMBEDDED: RefCell<Option<Embedded>> = const { RefCell::new(None) };
}

/// Open an embedded run on this thread with `input` as its stdin.
pub(crate) fn begin(input: &str) {
    EMBEDDED.with(|e| {
        *e.borrow_mut() = Some(Embedded {
            input: input.as_bytes().to_vec(),
            read_pos: 0,
            out: Vec::new(),
            exit: None,
        })
    });
}

/// Close the embedded run: its stdout, and the exit it ended with, if any.
pub(crate) fn end() -> (String, Option<(i32, String)>) {
    let run = EMBEDDED.with(|e| e.borrow_mut().take());
    match run {
        Some(r) => (String::from_utf8_lossy(&r.out).into_owned(), r.exit),
        None => (String::new(), None),
    }
}

/// Whether an embedded run is open on this thread.
pub(crate) fn embedded() -> bool {
    EMBEDDED.with(|e| e.borrow().is_some())
}

/// Write `bytes` to the program's stdout. In an embedded run, output after the
/// program exited is dropped, as it would never reach a dead process's stdout.
pub(crate) fn write_stdout(bytes: &[u8]) -> std::io::Result<()> {
    let embedded = EMBEDDED.with(|e| match e.borrow_mut().as_mut() {
        Some(run) => {
            if run.exit.is_none() {
                run.out.extend_from_slice(bytes);
            }
            true
        }
        None => false,
    });
    if embedded {
        return Ok(());
    }
    std::io::stdout().write_all(bytes)
}

/// Flush the process's stdout (nothing to do in an embedded run).
pub(crate) fn flush_stdout() -> std::io::Result<()> {
    std::io::stdout().flush()
}

/// One read from the program's stdin into `buf`: the byte count, 0 at end.
pub(crate) fn read_stdin(buf: &mut [u8]) -> std::io::Result<usize> {
    let embedded = EMBEDDED.with(|e| {
        e.borrow_mut().as_mut().map(|run| {
            let rest = &run.input[run.read_pos..];
            let n = rest.len().min(buf.len());
            buf[..n].copy_from_slice(&rest[..n]);
            run.read_pos += n;
            n
        })
    });
    match embedded {
        Some(n) => Ok(n),
        None => std::io::stdin().lock().read(buf),
    }
}

/// End the program with `status`, after writing `diagnostic` (empty for none)
/// to stderr. Outside an embedded run this exits the process; inside one it
/// records the first exit and halts `vm`, and the run reports it when it
/// returns.
pub(crate) fn exit(vm: &mut fusevm::VM, status: i32, diagnostic: String) {
    let recorded = EMBEDDED.with(|e| match e.borrow_mut().as_mut() {
        Some(run) => {
            run.exit.get_or_insert((status, diagnostic.clone()));
            true
        }
        None => false,
    });
    vm.request_halt();
    if !recorded {
        let _ = std::io::stdout().flush();
        eprint!("{diagnostic}");
        let _ = std::io::stderr().flush();
        std::process::exit(status);
    }
}
