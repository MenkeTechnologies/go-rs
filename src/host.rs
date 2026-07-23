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
/// `make([]T, n)` / `make(map[K]V)` — allocate a zeroed slice or empty map.
pub const GMAKE: u16 = 810;
/// `[]T{...}` slice composite literal.
pub const GSLICE_LIT: u16 = 811;
/// `map[K]V{...}` map composite literal (stack: k0,v0,k1,v1,...).
pub const GMAP_LIT: u16 = 812;
/// `x[i]` read — slice index (bounds-checked) or map lookup.
pub const GINDEX_GET: u16 = 813;
/// `x[i] = v` write — slice index or map insert.
pub const GINDEX_SET: u16 = 814;
/// `len(x)` — slice/map/string length.
pub const GLEN: u16 = 815;
/// `cap(x)` — slice capacity (its length in this model).
pub const GCAP: u16 = 816;
/// `append(s, elems...)` — extend a slice, returning the (same) handle.
pub const GAPPEND: u16 = 817;
/// `delete(m, k)` — remove a map key.
pub const GDELETE: u16 = 818;
/// `T{...}` struct composite literal (stack: typeName, f0name,f0val, ...).
pub const GSTRUCT_NEW: u16 = 820;
/// `s.field` read on a struct.
pub const GFIELD_GET: u16 = 821;
/// `s.field = v` write on a struct.
pub const GFIELD_SET: u16 = 822;
/// Deep-copy a struct value (Go struct value semantics on assign/pass/return).
pub const GSTRUCT_COPY: u16 = 823;
/// The range keys of a value as a slice: `0..len` for a slice/string, the keys
/// for a map. Lets `for … range` iterate slices and maps uniformly.
pub const GRANGE_KEYS: u16 = 824;
/// The runtime type name of a struct value (drives interface method dispatch).
pub const GTYPEOF: u16 = 825;

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
    vm.register_builtin(GMAKE, b_make);
    vm.register_builtin(GSLICE_LIT, b_slice_lit);
    vm.register_builtin(GMAP_LIT, b_map_lit);
    vm.register_builtin(GINDEX_GET, b_index_get);
    vm.register_builtin(GINDEX_SET, b_index_set);
    vm.register_builtin(GLEN, b_len);
    vm.register_builtin(GCAP, b_cap);
    vm.register_builtin(GAPPEND, b_append);
    vm.register_builtin(GDELETE, b_delete);
    vm.register_builtin(GSTRUCT_NEW, b_struct_new);
    vm.register_builtin(GFIELD_GET, b_field_get);
    vm.register_builtin(GFIELD_SET, b_field_set);
    vm.register_builtin(GSTRUCT_COPY, b_struct_copy);
    vm.register_builtin(GRANGE_KEYS, b_range_keys);
    vm.register_builtin(GTYPEOF, b_typeof);
    stdlib::install(vm);
}

/// The runtime type name of a struct value, or `""` for a non-struct. Used by
/// the compiler's interface method dispatch (a runtime type-switch).
fn b_typeof(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    match args.first() {
        Some(Value::Obj(id)) => HEAP.with(|h| {
            let h = h.borrow();
            match h.get(*id as usize) {
                Some(HostObj::Struct { type_name, .. }) => Value::str(type_name.clone()),
                _ => Value::str(""),
            }
        }),
        _ => Value::str(""),
    }
}

/// The range keys of a value as a fresh slice: `0..len` for a slice or string,
/// the map's keys for a map. Drives `for … range` uniformly for both.
fn b_range_keys(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let keys: Vec<Value> = match args.first() {
        Some(Value::Str(s)) => (0..s.len() as i64).map(Value::Int).collect(),
        Some(Value::Obj(id)) => HEAP.with(|h| {
            let h = h.borrow();
            match h.get(*id as usize) {
                Some(HostObj::Slice(a)) => (0..a.len() as i64).map(Value::Int).collect(),
                Some(HostObj::Map(m)) => m.iter().map(|(k, _)| k.clone()).collect(),
                _ => Vec::new(),
            }
        }),
        _ => Vec::new(),
    };
    Value::Obj(heap_alloc(HostObj::Slice(keys)))
}

// ── host-owned heap for Go composite types ─────────────────────────────────
//
// `Value::Obj(id)` is an opaque handle into [`HEAP`]; slices and maps are Go
// reference types, so sharing a handle is exactly right. Structs are value
// types — the compiler emits a `GSTRUCT_COPY` on assignment / parameter bind /
// return so a struct handle is never aliased (Go copy semantics).

/// One object on the host-owned Go heap.
pub(crate) enum HostObj {
    /// A slice (also the backing array). Go slices are reference types.
    Slice(Vec<Value>),
    /// A map, insertion-ordered for stable iteration; keys compared by value.
    Map(Vec<(Value, Value)>),
    /// A struct: its type name and ordered `(field, value)` pairs.
    Struct {
        type_name: String,
        fields: Vec<(String, Value)>,
    },
}

thread_local! {
    /// The host-owned Go object heap. `Value::Obj(id)` indexes this slab; it
    /// grows per run and is cleared by [`heap_reset`] at the start of every
    /// program so handles never leak across runs.
    static HEAP: RefCell<Vec<HostObj>> = const { RefCell::new(Vec::new()) };
}

/// Clear the object heap. Called at the start of each program run.
pub fn heap_reset() {
    HEAP.with(|h| h.borrow_mut().clear());
}

/// Allocate `obj` on the heap and return its handle.
fn heap_alloc(obj: HostObj) -> u32 {
    HEAP.with(|h| {
        let mut h = h.borrow_mut();
        let id = h.len() as u32;
        h.push(obj);
        id
    })
}

/// Whether two values are equal as Go map keys (comparable kinds only).
fn key_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        _ => a.to_float() == b.to_float(),
    }
}

/// `make([]T, n)` (2 args: kind-tag, n) or `make(map[K]V)` (1 arg: kind-tag).
/// The kind tag is a string: "slice" or "map". A slice is zero-filled with the
/// element zero value passed as the last argument.
fn b_make(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let kind = args.first().map(go_str).unwrap_or_default();
    match kind.as_str() {
        "map" => Value::Obj(heap_alloc(HostObj::Map(Vec::new()))),
        _ => {
            let n = args.get(1).map(|v| v.to_int()).unwrap_or(0);
            let zero = args.get(2).cloned().unwrap_or(Value::Int(0));
            if n < 0 {
                ffi_fault(vm, format!("go-rs: makeslice: len out of range ({n})"));
                return Value::Undef;
            }
            Value::Obj(heap_alloc(HostObj::Slice(vec![zero; n as usize])))
        }
    }
}

/// `[]T{a, b, …}` — build a slice from the popped element values.
fn b_slice_lit(vm: &mut VM, argc: u8) -> Value {
    let elems = pop_args(vm, argc);
    Value::Obj(heap_alloc(HostObj::Slice(elems)))
}

/// `map[K]V{k0: v0, …}` — build a map from popped `k0,v0,k1,v1,…` pairs.
fn b_map_lit(vm: &mut VM, argc: u8) -> Value {
    let flat = pop_args(vm, argc);
    let mut pairs = Vec::with_capacity(flat.len() / 2);
    let mut it = flat.into_iter();
    while let (Some(k), Some(v)) = (it.next(), it.next()) {
        if let Some(slot) = pairs.iter_mut().find(|(ek, _)| key_eq(ek, &k)) {
            slot.1 = v;
        } else {
            pairs.push((k, v));
        }
    }
    Value::Obj(heap_alloc(HostObj::Map(pairs)))
}

/// `x[i]` — slice index (bounds-checked) or map lookup (zero value if absent).
fn b_index_get(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let recv = args.first().cloned().unwrap_or(Value::Undef);
    let key = args.get(1).cloned().unwrap_or(Value::Undef);
    let id = match recv {
        Value::Obj(id) => id,
        // Indexing a string yields the byte at that position (Go: byte value).
        Value::Str(ref s) => {
            let i = key.to_int();
            return match usize::try_from(i).ok().and_then(|i| s.as_bytes().get(i)) {
                Some(b) => Value::Int(*b as i64),
                None => {
                    ffi_fault(vm, format!("go-rs: index out of range [{i}]"));
                    Value::Undef
                }
            };
        }
        _ => {
            ffi_fault(vm, "go-rs: invalid index of nil".to_string());
            return Value::Undef;
        }
    };
    HEAP.with(|h| {
        let h = h.borrow();
        match h.get(id as usize) {
            Some(HostObj::Slice(a)) => {
                let i = key.to_int();
                match usize::try_from(i).ok().and_then(|i| a.get(i)) {
                    Some(v) => v.clone(),
                    None => {
                        ffi_fault(
                            vm,
                            format!("go-rs: index out of range [{i}] with length {}", a.len()),
                        );
                        Value::Undef
                    }
                }
            }
            Some(HostObj::Map(m)) => m
                .iter()
                .find(|(k, _)| key_eq(k, &key))
                .map(|(_, v)| v.clone())
                // Go returns the value type's zero value for a missing key; 0
                // covers the common numeric case (comma-ok form is a later wave).
                .unwrap_or(Value::Int(0)),
            _ => {
                ffi_fault(vm, "go-rs: invalid index target".to_string());
                Value::Undef
            }
        }
    })
}

/// `x[i] = v` — slice element write (bounds-checked) or map insert. Returns `v`.
fn b_index_set(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let recv = args.first().cloned().unwrap_or(Value::Undef);
    let key = args.get(1).cloned().unwrap_or(Value::Undef);
    let val = args.get(2).cloned().unwrap_or(Value::Undef);
    let id = match recv {
        Value::Obj(id) => id,
        _ => {
            ffi_fault(vm, "go-rs: assignment to entry in nil".to_string());
            return Value::Undef;
        }
    };
    let err = HEAP.with(|h| {
        let mut h = h.borrow_mut();
        match h.get_mut(id as usize) {
            Some(HostObj::Slice(a)) => {
                let i = key.to_int();
                match usize::try_from(i).ok().filter(|&i| i < a.len()) {
                    Some(i) => {
                        a[i] = val.clone();
                        None
                    }
                    None => Some(format!(
                        "go-rs: index out of range [{i}] with length {}",
                        a.len()
                    )),
                }
            }
            Some(HostObj::Map(m)) => {
                if let Some(slot) = m.iter_mut().find(|(k, _)| key_eq(k, &key)) {
                    slot.1 = val.clone();
                } else {
                    m.push((key.clone(), val.clone()));
                }
                None
            }
            _ => Some("go-rs: invalid assignment target".to_string()),
        }
    });
    match err {
        None => val,
        Some(msg) => {
            ffi_fault(vm, msg);
            Value::Undef
        }
    }
}

/// `len(x)` — slice/map element count or string byte length.
fn b_len(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    match args.first() {
        Some(Value::Str(s)) => Value::Int(s.len() as i64),
        Some(Value::Obj(id)) => HEAP.with(|h| {
            let h = h.borrow();
            match h.get(*id as usize) {
                Some(HostObj::Slice(a)) => Value::Int(a.len() as i64),
                Some(HostObj::Map(m)) => Value::Int(m.len() as i64),
                _ => Value::Int(0),
            }
        }),
        _ => Value::Int(0),
    }
}

/// `cap(x)` — the capacity of a slice (its length in this model).
fn b_cap(vm: &mut VM, argc: u8) -> Value {
    b_len(vm, argc)
}

/// `append(s, elems...)` — extend slice `s` in place and return its handle.
/// A nil slice (non-handle) is treated as empty, so `append(nil, x)` allocates.
fn b_append(vm: &mut VM, argc: u8) -> Value {
    let mut args = pop_args(vm, argc);
    if args.is_empty() {
        return Value::Undef;
    }
    let recv = args.remove(0);
    match recv {
        Value::Obj(id) => {
            let ok = HEAP.with(|h| {
                let mut h = h.borrow_mut();
                if let Some(HostObj::Slice(a)) = h.get_mut(id as usize) {
                    a.extend(args.iter().cloned());
                    true
                } else {
                    false
                }
            });
            if ok {
                Value::Obj(id)
            } else {
                ffi_fault(
                    vm,
                    "go-rs: first argument to append must be a slice".to_string(),
                );
                Value::Undef
            }
        }
        _ => Value::Obj(heap_alloc(HostObj::Slice(args))),
    }
}

/// `delete(m, k)` — remove key `k` from map `m` (no-op if absent).
fn b_delete(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let recv = args.first().cloned().unwrap_or(Value::Undef);
    let key = args.get(1).cloned().unwrap_or(Value::Undef);
    if let Value::Obj(id) = recv {
        HEAP.with(|h| {
            let mut h = h.borrow_mut();
            if let Some(HostObj::Map(m)) = h.get_mut(id as usize) {
                m.retain(|(k, _)| !key_eq(k, &key));
            }
        });
    }
    Value::Undef
}

/// `T{f0: v0, …}` — build a struct (stack: typeName, f0name, f0val, …).
fn b_struct_new(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let type_name = args.first().map(go_str).unwrap_or_default();
    let mut fields = Vec::new();
    let mut it = args.into_iter().skip(1);
    while let (Some(name), Some(val)) = (it.next(), it.next()) {
        fields.push((go_str(&name), val));
    }
    Value::Obj(heap_alloc(HostObj::Struct { type_name, fields }))
}

/// `s.field` read on a struct.
fn b_field_get(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let recv = args.first().cloned().unwrap_or(Value::Undef);
    let name = args.get(1).map(go_str).unwrap_or_default();
    let id = match recv {
        Value::Obj(id) => id,
        _ => {
            ffi_fault(vm, format!("go-rs: nil dereference reading `{name}`"));
            return Value::Undef;
        }
    };
    HEAP.with(|h| {
        let h = h.borrow();
        match h.get(id as usize) {
            Some(HostObj::Struct { fields, .. }) => fields
                .iter()
                .find(|(f, _)| *f == name)
                .map(|(_, v)| v.clone())
                .unwrap_or(Value::Undef),
            _ => {
                ffi_fault(vm, format!("go-rs: no field `{name}`"));
                Value::Undef
            }
        }
    })
}

/// `s.field = v` write on a struct. Returns `v`.
fn b_field_set(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let recv = args.first().cloned().unwrap_or(Value::Undef);
    let name = args.get(1).map(go_str).unwrap_or_default();
    let val = args.get(2).cloned().unwrap_or(Value::Undef);
    let id = match recv {
        Value::Obj(id) => id,
        _ => {
            ffi_fault(vm, format!("go-rs: nil dereference assigning `{name}`"));
            return Value::Undef;
        }
    };
    let ok = HEAP.with(|h| {
        let mut h = h.borrow_mut();
        match h.get_mut(id as usize) {
            Some(HostObj::Struct { fields, .. }) => {
                if let Some(slot) = fields.iter_mut().find(|(f, _)| *f == name) {
                    slot.1 = val.clone();
                } else {
                    fields.push((name.clone(), val.clone()));
                }
                true
            }
            _ => false,
        }
    });
    if ok {
        val
    } else {
        ffi_fault(vm, format!("go-rs: cannot assign field `{name}`"));
        Value::Undef
    }
}

/// Deep-copy a struct value (Go value semantics). Non-struct values pass
/// through unchanged (slices/maps are reference types and must NOT be copied).
fn b_struct_copy(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let v = args.first().cloned().unwrap_or(Value::Undef);
    match v {
        Value::Obj(id) => {
            let cloned = HEAP.with(|h| {
                let h = h.borrow();
                match h.get(id as usize) {
                    Some(HostObj::Struct { type_name, fields }) => Some(HostObj::Struct {
                        type_name: type_name.clone(),
                        fields: fields.clone(),
                    }),
                    _ => None,
                }
            });
            match cloned {
                Some(obj) => Value::Obj(heap_alloc(obj)),
                None => v, // slice/map/other: reference type, share the handle
            }
        }
        other => other,
    }
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

/// Render a value the way Go's `fmt` `%v` verb does. Composite values (slices,
/// maps, structs) are looked up on the heap and formatted with Go's bracket /
/// `map[…]` / `{…}` conventions.
pub fn go_str(v: &Value) -> String {
    match v {
        Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => format_float(*f),
        Value::Str(s) => s.as_str().to_string(),
        Value::Undef => "<nil>".to_string(),
        Value::Obj(id) => obj_str(*id),
        other => other.as_str_cow().into_owned(),
    }
}

/// Format a heap object the way Go's `%v` does: `[e0 e1 …]` for a slice,
/// `map[k0:v0 …]` (keys sorted, as Go's fmt does) for a map, `{f0 f1 …}` for a
/// struct.
fn obj_str(id: u32) -> String {
    HEAP.with(|h| {
        let h = h.borrow();
        match h.get(id as usize) {
            Some(HostObj::Slice(a)) => {
                let parts: Vec<String> = a.iter().map(go_str).collect();
                format!("[{}]", parts.join(" "))
            }
            Some(HostObj::Map(m)) => {
                let mut parts: Vec<String> = m
                    .iter()
                    .map(|(k, v)| format!("{}:{}", go_str(k), go_str(v)))
                    .collect();
                parts.sort();
                format!("map[{}]", parts.join(" "))
            }
            Some(HostObj::Struct { fields, .. }) => {
                let parts: Vec<String> = fields.iter().map(|(_, v)| go_str(v)).collect();
                format!("{{{}}}", parts.join(" "))
            }
            None => "<nil>".to_string(),
        }
    })
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

/// A minimal `strings` and `strconv` standard library. Each exported function
/// is a numbered builtin the compiler dispatches `strings.X` / `strconv.X` calls
/// to. Split/Fields return heap slices; Join reads one.
pub mod stdlib {
    use super::{go_str, heap_alloc, pop_args, HostObj, HEAP};
    use fusevm::{Value, VM};

    // strings.*
    pub const TO_UPPER: u16 = 830;
    pub const TO_LOWER: u16 = 831;
    pub const CONTAINS: u16 = 832;
    pub const HAS_PREFIX: u16 = 833;
    pub const HAS_SUFFIX: u16 = 834;
    pub const TRIM_SPACE: u16 = 835;
    pub const SPLIT: u16 = 836;
    pub const JOIN: u16 = 837;
    pub const REPEAT: u16 = 838;
    pub const INDEX: u16 = 839;
    pub const REPLACE_ALL: u16 = 840;
    pub const FIELDS: u16 = 841;
    // strconv.*
    pub const ITOA: u16 = 850;
    pub const ATOI: u16 = 851;

    /// Resolve `pkg.func` to a stdlib builtin id, or `None` if unknown.
    pub fn resolve(pkg: &str, func: &str) -> Option<u16> {
        Some(match (pkg, func) {
            ("strings", "ToUpper") => TO_UPPER,
            ("strings", "ToLower") => TO_LOWER,
            ("strings", "Contains") => CONTAINS,
            ("strings", "HasPrefix") => HAS_PREFIX,
            ("strings", "HasSuffix") => HAS_SUFFIX,
            ("strings", "TrimSpace") => TRIM_SPACE,
            ("strings", "Split") => SPLIT,
            ("strings", "Join") => JOIN,
            ("strings", "Repeat") => REPEAT,
            ("strings", "Index") => INDEX,
            ("strings", "ReplaceAll") => REPLACE_ALL,
            ("strings", "Fields") => FIELDS,
            ("strconv", "Itoa") => ITOA,
            ("strconv", "Atoi") => ATOI,
            _ => return None,
        })
    }

    pub fn install(vm: &mut VM) {
        vm.register_builtin(TO_UPPER, |vm, a| s1(vm, a, |s| s.to_uppercase()));
        vm.register_builtin(TO_LOWER, |vm, a| s1(vm, a, |s| s.to_lowercase()));
        vm.register_builtin(TRIM_SPACE, |vm, a| s1(vm, a, |s| s.trim().to_string()));
        vm.register_builtin(CONTAINS, |vm, a| b2(vm, a, |s, p| s.contains(p)));
        vm.register_builtin(HAS_PREFIX, |vm, a| b2(vm, a, |s, p| s.starts_with(p)));
        vm.register_builtin(HAS_SUFFIX, |vm, a| b2(vm, a, |s, p| s.ends_with(p)));
        vm.register_builtin(INDEX, b_index);
        vm.register_builtin(REPEAT, b_repeat);
        vm.register_builtin(REPLACE_ALL, b_replace_all);
        vm.register_builtin(SPLIT, b_split);
        vm.register_builtin(FIELDS, b_fields);
        vm.register_builtin(JOIN, b_join);
        vm.register_builtin(ITOA, b_itoa);
        vm.register_builtin(ATOI, b_atoi);
    }

    /// A one-string-arg → string builtin.
    fn s1(vm: &mut VM, argc: u8, f: impl Fn(&str) -> String) -> Value {
        let args = pop_args(vm, argc);
        Value::str(f(&args.first().map(go_str).unwrap_or_default()))
    }

    /// A two-string-arg → bool builtin.
    fn b2(vm: &mut VM, argc: u8, f: impl Fn(&str, &str) -> bool) -> Value {
        let args = pop_args(vm, argc);
        let s = args.first().map(go_str).unwrap_or_default();
        let p = args.get(1).map(go_str).unwrap_or_default();
        Value::bool(f(&s, &p))
    }

    fn b_index(vm: &mut VM, argc: u8) -> Value {
        let args = pop_args(vm, argc);
        let s = args.first().map(go_str).unwrap_or_default();
        let sub = args.get(1).map(go_str).unwrap_or_default();
        Value::Int(s.find(&sub).map(|b| b as i64).unwrap_or(-1))
    }

    fn b_repeat(vm: &mut VM, argc: u8) -> Value {
        let args = pop_args(vm, argc);
        let s = args.first().map(go_str).unwrap_or_default();
        let n = args.get(1).map(|v| v.to_int()).unwrap_or(0).max(0) as usize;
        Value::str(s.repeat(n))
    }

    fn b_replace_all(vm: &mut VM, argc: u8) -> Value {
        let args = pop_args(vm, argc);
        let s = args.first().map(go_str).unwrap_or_default();
        let old = args.get(1).map(go_str).unwrap_or_default();
        let new = args.get(2).map(go_str).unwrap_or_default();
        Value::str(if old.is_empty() {
            s
        } else {
            s.replace(&old, &new)
        })
    }

    fn b_split(vm: &mut VM, argc: u8) -> Value {
        let args = pop_args(vm, argc);
        let s = args.first().map(go_str).unwrap_or_default();
        let sep = args.get(1).map(go_str).unwrap_or_default();
        let parts: Vec<Value> = if sep.is_empty() {
            s.chars().map(|c| Value::str(c.to_string())).collect()
        } else {
            s.split(&sep).map(Value::str).collect()
        };
        Value::Obj(heap_alloc(HostObj::Slice(parts)))
    }

    fn b_fields(vm: &mut VM, argc: u8) -> Value {
        let args = pop_args(vm, argc);
        let s = args.first().map(go_str).unwrap_or_default();
        let parts: Vec<Value> = s.split_whitespace().map(Value::str).collect();
        Value::Obj(heap_alloc(HostObj::Slice(parts)))
    }

    fn b_join(vm: &mut VM, argc: u8) -> Value {
        let args = pop_args(vm, argc);
        let sep = args.get(1).map(go_str).unwrap_or_default();
        let joined = match args.first() {
            Some(Value::Obj(id)) => HEAP.with(|h| {
                let h = h.borrow();
                match h.get(*id as usize) {
                    Some(HostObj::Slice(a)) => a.iter().map(go_str).collect::<Vec<_>>().join(&sep),
                    _ => String::new(),
                }
            }),
            _ => String::new(),
        };
        Value::str(joined)
    }

    /// `strconv.Itoa(n)` — integer to decimal string.
    fn b_itoa(vm: &mut VM, argc: u8) -> Value {
        let args = pop_args(vm, argc);
        Value::str(args.first().map(|v| v.to_int()).unwrap_or(0).to_string())
    }

    /// `strconv.Atoi(s)` — parse a decimal string; go-rs returns 0 on failure
    /// (the paired error is a later wave).
    fn b_atoi(vm: &mut VM, argc: u8) -> Value {
        let args = pop_args(vm, argc);
        let s = args.first().map(go_str).unwrap_or_default();
        Value::Int(s.trim().parse::<i64>().unwrap_or(0))
    }
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
