//! Host builtins and the strict numeric hook for go-rs.
//!
//! fusevm runs the lowered chunk; this module supplies the runtime behavior the
//! bytecode can't express directly: the `fmt` print family (`Println`/`Print`/
//! `Printf`) and the Go builtins `println`/`print`, plus a [`numeric_hook`] that
//! gives `+` its string-concatenation overload and `<`/`==`/… their string
//! ordering. Values render with Go's `fmt` `%v` rules ([`go_str`]).

use fusevm::{NumOp, Value, VM};
use std::cell::RefCell;

/// `fmt.Println` — space-separated operands, trailing newline, to stdout.
pub const GPRINTLN: u16 = 800;
/// `fmt.Print` — operands with Go's between-non-strings spacing, to stdout.
pub const GPRINT: u16 = 801;
/// `fmt.Printf` — format string + args, to stdout.
pub const GPRINTF: u16 = 802;
/// Go builtin `println` — space-separated, trailing newline, to stderr.
pub const GEPRINTLN: u16 = 803;
/// Go builtin `print` — no spacing, to stderr.
pub const GEPRINT: u16 = 804;
/// `__rust_compile("<base64>", line)` — compile an inline `rust {}` block.
pub const GFFI_COMPILE: u16 = 805;
/// FFI dispatch: call an exported inline-Rust symbol by name.
pub const GFFI_CALL: u16 = 806;
/// `--dap` per-statement line marker (only emitted by `compile_debug`).
pub const DBG_LINE: u16 = 807;

/// Register every go-rs builtin on a VM. This is the single install choke point
/// later waves (slices, maps, `strings`/`strconv`, structs) grow into.
pub fn install(vm: &mut VM) {
    vm.register_builtin(GPRINTLN, b_println);
    vm.register_builtin(GPRINT, b_print);
    vm.register_builtin(GPRINTF, b_printf);
    vm.register_builtin(GEPRINTLN, b_eprintln);
    vm.register_builtin(GEPRINT, b_eprint);
    vm.register_builtin(GFFI_COMPILE, b_ffi_compile);
    vm.register_builtin(GFFI_CALL, b_ffi_call);
}

/// Install the builtins plus the debug line-marker used by `go --dap`. The
/// marker fires synchronously at each statement and delegates to the DAP server,
/// which pauses in place on a breakpoint or step target.
pub fn install_debug(vm: &mut VM) {
    install(vm);
    vm.register_builtin(DBG_LINE, b_dbg_line);
}

thread_local! {
    /// Set by an inline-Rust FFI fault (compile error, call error, or an
    /// unresolved export). A builtin cannot return a `Result`, so it stashes the
    /// message here and halts the VM; [`crate::run_str`] reads it after
    /// `VM::run` returns and surfaces it as a `go-rs:` error.
    static FFI_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Take and clear any pending FFI-fault message.
pub fn take_ffi_error() -> Option<String> {
    FFI_ERROR.with(|e| e.borrow_mut().take())
}

/// Record an FFI fault and halt the VM; the message surfaces after the run.
fn ffi_fault(vm: &mut VM, msg: impl Into<String>) {
    FFI_ERROR.with(|e| *e.borrow_mut() = Some(msg.into()));
    vm.request_halt();
}

/// `__rust_compile("<base64>", line)` builtin: pop the base64-encoded
/// `rust { ... }` block body, compile it to a cdylib, and register its exports.
fn b_ffi_compile(vm: &mut VM, argc: u8) -> Value {
    // The compiler emits `(base64, line)`; the base64 body is the deepest arg.
    let args = pop_args(vm, argc);
    let b64 = args.first().map(go_str).unwrap_or_default();
    if let Err(e) = fusevm::ffi::compile_and_register(&b64) {
        ffi_fault(vm, format!("go-rs: rust {{}} block: {e}"));
    }
    Value::Undef
}

/// `name(args...)` FFI dispatch: pop the function name (top of stack) and its
/// `argc - 1` arguments, call the exported symbol via `fusevm::ffi`, and return
/// its result.
fn b_ffi_call(vm: &mut VM, argc: u8) -> Value {
    let name = vm
        .stack
        .pop()
        .map(|v| v.as_str_cow().into_owned())
        .unwrap_or_default();
    let n = argc.saturating_sub(1) as usize;
    let mut args = Vec::with_capacity(n);
    for _ in 0..n {
        args.push(vm.stack.pop().unwrap_or(Value::Undef));
    }
    args.reverse();
    match fusevm::ffi::try_call(&name, &args) {
        Some(Ok(v)) => v,
        Some(Err(e)) => {
            ffi_fault(vm, format!("go-rs: rust FFI call {name}: {e}"));
            Value::Undef
        }
        None => {
            ffi_fault(vm, format!("go-rs: undefined: {name}"));
            Value::Undef
        }
    }
}

/// The `DBG_LINE` marker builtin: hand control to the DAP server for this line,
/// then return nil (popped by the trailing `Op::Pop` the compiler emits).
fn b_dbg_line(vm: &mut VM, _argc: u8) -> Value {
    crate::dap::on_debug_line(vm);
    Value::Undef
}

/// Pop `argc` values off the VM stack, restoring source (left-to-right) order.
fn pop_args(vm: &mut VM, argc: u8) -> Vec<Value> {
    let mut v = Vec::with_capacity(argc as usize);
    for _ in 0..argc {
        v.push(vm.stack.pop().unwrap_or(Value::Undef));
    }
    v.reverse();
    v
}

/// Render a value the way Go's `fmt` `%v` verb does.
pub fn go_str(v: &Value) -> String {
    match v {
        Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => format_float(*f),
        Value::Str(s) => s.as_str().to_string(),
        Value::Undef => "<nil>".to_string(),
        other => other.as_str_cow().into_owned(),
    }
}

/// Go prints floats via `strconv.FormatFloat(f, 'g', -1, 64)`: shortest exact
/// decimal, whole values without a fractional part (`3`, not `3.0`), and
/// `+Inf`/`-Inf`/`NaN` for the non-finite cases.
fn format_float(f: f64) -> String {
    if f.is_nan() {
        "NaN".to_string()
    } else if f.is_infinite() {
        if f < 0.0 { "-Inf" } else { "+Inf" }.to_string()
    } else {
        // Rust's `{}` for f64 already yields the shortest round-tripping decimal
        // and omits a trailing `.0`, matching Go's `%v` for the common range.
        format!("{f}")
    }
}

fn b_println(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let text: Vec<String> = args.iter().map(go_str).collect();
    println!("{}", text.join(" "));
    Value::Undef
}

fn b_print(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    print!("{}", go_print_spacing(&args));
    Value::Undef
}

fn b_printf(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    print!("{}", sprintf(&args));
    Value::Undef
}

fn b_eprintln(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let text: Vec<String> = args.iter().map(go_str).collect();
    eprintln!("{}", text.join(" "));
    Value::Undef
}

fn b_eprint(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    eprint!("{}", go_print_spacing(&args));
    Value::Undef
}

/// Go's `Print`/builtin-`print` spacing: a space is inserted between two
/// operands only when neither is a string.
fn go_print_spacing(args: &[Value]) -> String {
    let mut out = String::new();
    for (i, v) in args.iter().enumerate() {
        if i > 0 {
            let prev_str = matches!(args[i - 1], Value::Str(_));
            let cur_str = matches!(v, Value::Str(_));
            if !prev_str && !cur_str {
                out.push(' ');
            }
        }
        out.push_str(&go_str(v));
    }
    out
}

/// A minimal `fmt.Printf`: the first argument is the format string; verbs
/// `%v %d %s %f %t %q %%` consume successive arguments. Flags, width, and
/// precision are skipped (slice 1). An unmatched verb falls back to `%v`.
fn sprintf(args: &[Value]) -> String {
    let fmt = args.first().map(go_str).unwrap_or_default();
    let mut out = String::new();
    let mut rest = args.iter().skip(1);
    let bytes = fmt.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            out.push(bytes[i] as char);
            i += 1;
            continue;
        }
        i += 1;
        if i >= bytes.len() {
            out.push('%');
            break;
        }
        // Skip flags/width/precision until the verb letter.
        while i < bytes.len() && matches!(bytes[i], b'+' | b'-' | b'#' | b' ' | b'0' | b'.') {
            i += 1;
        }
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let verb = bytes[i] as char;
        i += 1;
        match verb {
            '%' => out.push('%'),
            't' => out.push_str(&rest.next().map(go_str).unwrap_or_default()),
            'q' => {
                let s = rest.next().map(go_str).unwrap_or_default();
                out.push('"');
                out.push_str(&s);
                out.push('"');
            }
            'f' => {
                let v = rest.next().cloned().unwrap_or(Value::Undef);
                out.push_str(&format!("{:.6}", v.to_float()));
            }
            // %v, %d, %s and anything else: Go's default rendering.
            _ => out.push_str(&rest.next().map(go_str).unwrap_or_default()),
        }
    }
    out
}

/// Strict numeric hook: fusevm delegates here when an operand of an arithmetic
/// or comparison op is non-numeric (a string). Implements Go's `+` string
/// concatenation and string ordering; every other arithmetic op on a string is
/// a type error, reported rather than coerced (Go rejects `"a" - 1`).
pub fn numeric_hook(op: NumOp, a: &Value, b: &Value) -> Result<Value, String> {
    match op {
        NumOp::Add => Ok(Value::str(format!("{}{}", go_str(a), go_str(b)))),
        NumOp::Eq => Ok(Value::bool(go_str(a) == go_str(b))),
        NumOp::Ne => Ok(Value::bool(go_str(a) != go_str(b))),
        NumOp::Lt => Ok(Value::bool(go_str(a) < go_str(b))),
        NumOp::Gt => Ok(Value::bool(go_str(a) > go_str(b))),
        NumOp::Le => Ok(Value::bool(go_str(a) <= go_str(b))),
        NumOp::Ge => Ok(Value::bool(go_str(a) >= go_str(b))),
        NumOp::Sub | NumOp::Mul | NumOp::Div | NumOp::Mod | NumOp::Pow => Err(format!(
            "go-rs: invalid operation: operator {op:?} not defined on `{}`",
            go_str(a)
        )),
        NumOp::Neg => Err(format!(
            "go-rs: invalid operation: unary `-` not defined on `{}`",
            go_str(a)
        )),
    }
}
