//! Native AOT for go-rs via `fusevm::aot` (`go build`).
//!
//! Mirrors the pythonrs/elisprs approach, but simpler: go-rs lowers the whole
//! program (main + funcs + methods + lambdas) into a single self-contained
//! `fusevm::Chunk` with `sub_entries`, so there is no host function-table image
//! to embed — the chunk carries everything. `go build`:
//!   1. compiles the source to a `Chunk`,
//!   2. emits a relocatable object with `fusevm::aot::compile_object`,
//!   3. links it against the go-rs runtime staticlib (`libgors.a`, which carries
//!      fusevm's AOT runtime + `fusevm_aot_register_builtins` below) and a tiny
//!      C entry into a standalone executable.
//!
//! Limitation: the AOT entry runs the chunk on a single VM (`run_chunk_native`),
//! not the cooperative scheduler, so programs using goroutines/channels/`select`
//! must use `go run`. `go build` targets the compute/CLI subset.

use fusevm::VM;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Compile Go source to a standalone native executable at `out`.
pub fn build(src: &str, out: &Path) -> Result<(), String> {
    let chunk = crate::compile(src)?;
    // The AOT entry runs a single VM, not the scheduler; refuse concurrency
    // programs with a clear message rather than emit a binary that deadlocks.
    if chunk.ops.iter().any(|op| {
        matches!(
            op,
            fusevm::Op::Go(..)
                | fusevm::Op::ChanMake
                | fusevm::Op::ChanSend
                | fusevm::Op::ChanRecv
                | fusevm::Op::ChanClose
                | fusevm::Op::Select(..)
        )
    }) {
        return Err(
            "go-rs: build: programs using goroutines/channels/select need the scheduler — use `go run`"
                .to_string(),
        );
    }
    let obj = std::env::temp_dir().join(format!("gors_aot_{}.o", std::process::id()));
    fusevm::aot::compile_object(&chunk, &obj).map_err(|e| format!("go-rs: build: {e}"))?;

    let main_c = std::env::temp_dir().join(format!("gors_aot_{}.c", std::process::id()));
    std::fs::write(
        &main_c,
        "extern long fusevm_aot_run_embedded(void);\n\
         int main(void) { return (int)fusevm_aot_run_embedded(); }\n",
    )
    .map_err(|e| format!("go-rs: build: {e}"))?;

    let lib = staticlib_path()?;
    let mut cmd = std::process::Command::new("cc");
    cmd.arg(&main_c).arg(&obj).arg(&lib).arg("-o").arg(out);
    if cfg!(target_os = "macos") {
        cmd.args([
            "-framework",
            "CoreFoundation",
            "-framework",
            "Security",
            "-liconv",
            "-lc++",
        ]);
    } else {
        cmd.args(["-lpthread", "-ldl", "-lm", "-lrt"]);
    }
    let status = cmd.status().map_err(|e| format!("go-rs: cc: {e}"))?;
    let _ = std::fs::remove_file(&obj);
    let _ = std::fs::remove_file(&main_c);
    if !status.success() {
        return Err(format!(
            "go-rs: build: link failed (cc exit {:?})",
            status.code()
        ));
    }
    Ok(())
}

/// The default output path for `go build <file.go>`: the source basename without
/// its extension (matching `go build`), in the current directory.
pub fn default_output(file: &str) -> PathBuf {
    let stem = Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "a.out".to_string());
    PathBuf::from(stem)
}

/// Locate `libgors.a` (a sibling of the running `go` binary, or `$GORS_STATICLIB`).
fn staticlib_path() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("GORS_STATICLIB") {
        return Ok(PathBuf::from(p));
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let lib = exe.parent().ok_or("no exe dir")?.join("libgors.a");
    if lib.exists() {
        Ok(lib)
    } else {
        Err(format!(
            "go-rs: build: libgors.a not found next to {}; build the staticlib (cargo build) or set GORS_STATICLIB",
            exe.display()
        ))
    }
}

/// The AOT runtime hook: install the go-rs builtins + strict numeric hook and
/// reset the composite-type heap before the embedded chunk runs. This is the
/// link symbol fusevm's AOT entry calls for a standalone go-rs binary.
///
/// # Safety
/// `vm` must be a valid, exclusively-borrowable pointer (fusevm's AOT entry
/// passes one).
#[no_mangle]
pub unsafe extern "C" fn fusevm_aot_register_builtins(vm: *mut VM) {
    let vm = unsafe { &mut *vm };
    crate::host::heap_reset();
    crate::host::install(vm);
    vm.set_numeric_hook(Arc::new(crate::host::numeric_hook));
}
