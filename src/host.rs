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
/// Go 1.21 builtin `min(a, b, …)` — the smallest of the (ordered) arguments.
pub const GMIN: u16 = 826;
/// Go 1.21 builtin `max(a, b, …)` — the largest of the (ordered) arguments.
pub const GMAX: u16 = 827;
/// Build a closure: stack `[cap0, …, capN, lambda_id]`; pushes a closure value.
pub const GCLOSURE_NEW: u16 = 828;
/// Read a closure's captured value by index: stack `[closure, idx]`.
pub const GCLOSURE_GET: u16 = 829;
/// Read a closure's target subroutine name-index (for `Op::CallDynamic`).
pub const GCLOSURE_NAMEIDX: u16 = 819;
/// Push a new (empty) defer frame — one per invocation of a function that has
/// `defer` statements. (IDs 830–880 belong to the `stdlib` submodule.)
pub const GDEFER_ENTER: u16 = 881;
/// `[closure]` → push a deferred closure onto the current defer frame.
pub const GDEFER_PUSH: u16 = 882;
/// → `Int` count of deferred closures in the current frame (drives the drain).
pub const GDEFER_LEN: u16 = 883;
/// → pop and return the most-recently-deferred closure of the current frame.
pub const GDEFER_POP: u16 = 884;
/// Pop the (drained) defer frame.
pub const GDEFER_LEAVE: u16 = 885;
/// `[value]` → record a panic; execution unwinds to the current function's
/// defer drain (a deferred `recover()` may cancel it).
pub const GPANIC: u16 = 886;
/// → `Bool` whether a panic is currently propagating (drives unwind checks).
pub const GPANIC_ACTIVE: u16 = 887;
/// → the propagating panic value and clear it (nil if none) — Go's `recover()`.
pub const GRECOVER: u16 = 888;
/// If a panic is still propagating at program end, print it and exit non-zero.
pub const GPANIC_FINISH: u16 = 889;
/// `[value]` → a heap cell boxing `value`, for a variable captured by reference.
pub const GCELL_NEW: u16 = 890;
/// `[cell]` → read a boxed variable's current value.
pub const GCELL_GET: u16 = 891;
/// `[value, cell]` → store into a boxed variable (shared with its closures).
pub const GCELL_SET: u16 = 892;
/// `fmt.Sprintf(format, …)` — format and return the string (no output).
pub const GSPRINTF: u16 = 893;
/// `fmt.Sprint(…)` — concatenate operands (Go spacing) and return the string.
pub const GSPRINT: u16 = 894;
/// `fmt.Sprintln(…)` — like `Sprint` with spaces + trailing newline.
pub const GSPRINTLN: u16 = 895;
/// `s[low:high]` — a sub-slice/substring: stack `[recv, low, high]`.
pub const GSLICE_SUB: u16 = 896;

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
    vm.register_builtin(GMIN, b_min);
    vm.register_builtin(GMAX, b_max);
    vm.register_builtin(GCLOSURE_NEW, b_closure_new);
    vm.register_builtin(GCLOSURE_GET, b_closure_get);
    vm.register_builtin(GCLOSURE_NAMEIDX, b_closure_nameidx);
    vm.register_builtin(GDEFER_ENTER, b_defer_enter);
    vm.register_builtin(GDEFER_PUSH, b_defer_push);
    vm.register_builtin(GDEFER_LEN, b_defer_len);
    vm.register_builtin(GDEFER_POP, b_defer_pop);
    vm.register_builtin(GDEFER_LEAVE, b_defer_leave);
    vm.register_builtin(GPANIC, b_panic);
    vm.register_builtin(GPANIC_ACTIVE, b_panic_active);
    vm.register_builtin(GRECOVER, b_recover);
    vm.register_builtin(GPANIC_FINISH, b_panic_finish);
    vm.register_builtin(GCELL_NEW, b_cell_new);
    vm.register_builtin(GCELL_GET, b_cell_get);
    vm.register_builtin(GCELL_SET, b_cell_set);
    vm.register_builtin(GSPRINTF, b_sprintf);
    vm.register_builtin(GSPRINT, b_sprint);
    vm.register_builtin(GSPRINTLN, b_sprintln);
    vm.register_builtin(GSLICE_SUB, b_slice_sub);
    stdlib::install(vm);
}

/// `s[low:high]` on a slice or string: stack `[recv, low, high]`. Returns a new
/// slice (a copy — go-rs sub-slices don't share the parent's backing array) or a
/// substring. `low`/`high` of `-1` mean "omitted" (0 / len).
fn b_slice_sub(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let recv = args.first().cloned().unwrap_or(Value::Undef);
    let lo_raw = args.get(1).map(Value::to_int).unwrap_or(-1);
    let hi_raw = args.get(2).map(Value::to_int).unwrap_or(-1);
    match recv {
        Value::Obj(id) => HEAP.with(|h| {
            let sub = match h.borrow().get(id as usize) {
                Some(HostObj::Slice(a)) => {
                    let len = a.len() as i64;
                    let lo = if lo_raw < 0 { 0 } else { lo_raw }.clamp(0, len) as usize;
                    let hi = if hi_raw < 0 { len } else { hi_raw }.clamp(0, len) as usize;
                    a.get(lo..hi.max(lo))
                        .map(|s| s.to_vec())
                        .unwrap_or_default()
                }
                _ => return Value::Undef,
            };
            Value::Obj(heap_alloc(HostObj::Slice(sub)))
        }),
        Value::Str(s) => {
            // Byte-indexed substring, matching Go's string slicing.
            let bytes = s.as_bytes();
            let len = bytes.len() as i64;
            let lo = if lo_raw < 0 { 0 } else { lo_raw }.clamp(0, len) as usize;
            let hi = if hi_raw < 0 { len } else { hi_raw }.clamp(0, len) as usize;
            let slice = bytes.get(lo..hi.max(lo)).unwrap_or(&[]);
            Value::str(String::from_utf8_lossy(slice).into_owned())
        }
        _ => Value::Undef,
    }
}

/// `fmt.Sprintf(format, …)` — the formatted string (no output).
fn b_sprintf(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    Value::str(sprintf(&args))
}

/// `fmt.Sprint(…)` — operands concatenated with Go's between-non-strings spacing.
fn b_sprint(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    Value::str(go_print_spacing(&args))
}

/// `fmt.Sprintln(…)` — operands space-separated with a trailing newline.
fn b_sprintln(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let text: Vec<String> = args.iter().map(go_str).collect();
    Value::str(format!("{}\n", text.join(" ")))
}

/// `[value]` → a fresh heap cell boxing `value` (a by-reference-captured var).
fn b_cell_new(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let v = args.into_iter().next().unwrap_or(Value::Undef);
    Value::Obj(heap_alloc(HostObj::Cell(v)))
}

/// `[cell]` → the boxed value.
fn b_cell_get(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    match args.first() {
        Some(Value::Obj(id)) => HEAP.with(|h| match h.borrow().get(*id as usize) {
            Some(HostObj::Cell(v)) => v.clone(),
            _ => Value::Undef,
        }),
        _ => Value::Undef,
    }
}

/// `[value, cell]` → store `value` into the shared cell (writes reach closures).
fn b_cell_set(vm: &mut VM, argc: u8) -> Value {
    let mut args = pop_args(vm, argc);
    // Stack order is [value, cell]; `cell` is the top (last) argument.
    let cell = args.pop().unwrap_or(Value::Undef);
    let value = args.pop().unwrap_or(Value::Undef);
    if let Value::Obj(id) = cell {
        HEAP.with(|h| {
            if let Some(HostObj::Cell(slot)) = h.borrow_mut().get_mut(id as usize) {
                *slot = value;
            }
        });
    }
    Value::Undef
}

/// `[value]` → begin a panic: store the value; unwinding is driven by the
/// compiler (jump to the defer drain, propagate past calls while active).
fn b_panic(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let v = args.into_iter().next().unwrap_or(Value::Undef);
    PANIC.with(|p| *p.borrow_mut() = Some(v));
    Value::Undef
}

/// → whether a panic is propagating (an unwind check after each call).
fn b_panic_active(_vm: &mut VM, _argc: u8) -> Value {
    Value::bool(PANIC.with(|p| p.borrow().is_some()))
}

/// Go's `recover()`: return the propagating panic value and stop the panic, or
/// nil when nothing is panicking.
fn b_recover(_vm: &mut VM, _argc: u8) -> Value {
    PANIC
        .with(|p| p.borrow_mut().take())
        .unwrap_or(Value::Undef)
}

/// At program end, a still-propagating panic is fatal: print it like Go's first
/// line (`panic: <value>`) on stderr and exit with status 2. (The goroutine
/// stack trace Go prints below that line is not reproduced.)
fn b_panic_finish(_vm: &mut VM, _argc: u8) -> Value {
    if let Some(v) = PANIC.with(|p| p.borrow_mut().take()) {
        eprintln!("panic: {}", go_str(&v));
        std::process::exit(2);
    }
    Value::Undef
}

/// Push a fresh defer frame at the start of a function that has `defer`s.
fn b_defer_enter(_vm: &mut VM, _argc: u8) -> Value {
    DEFERS.with(|d| d.borrow_mut().push(Vec::new()));
    Value::Undef
}

/// `[closure]` → record a deferred closure in the current frame (LIFO order).
fn b_defer_push(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    if let Some(c) = args.into_iter().next() {
        DEFERS.with(|d| {
            if let Some(frame) = d.borrow_mut().last_mut() {
                frame.push(c);
            }
        });
    }
    Value::Undef
}

/// → the number of deferred closures still to run in the current frame.
fn b_defer_len(_vm: &mut VM, _argc: u8) -> Value {
    Value::Int(DEFERS.with(|d| d.borrow().last().map(|f| f.len()).unwrap_or(0)) as i64)
}

/// → pop the most-recently-deferred closure of the current frame (LIFO).
fn b_defer_pop(_vm: &mut VM, _argc: u8) -> Value {
    DEFERS.with(|d| {
        d.borrow_mut()
            .last_mut()
            .and_then(|f| f.pop())
            .unwrap_or(Value::Undef)
    })
}

/// Drop the drained defer frame on function exit.
fn b_defer_leave(_vm: &mut VM, _argc: u8) -> Value {
    DEFERS.with(|d| {
        d.borrow_mut().pop();
    });
    Value::Undef
}

/// `[closure]` → the closure's target subroutine name-index (drives dynamic
/// dispatch of a function value via `Op::CallDynamic`).
fn b_closure_nameidx(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    match args.first() {
        Some(Value::Obj(id)) => HEAP.with(|h| {
            let h = h.borrow();
            match h.get(*id as usize) {
                Some(HostObj::Closure { name_idx, .. }) => Value::Int(*name_idx),
                _ => Value::Int(-1),
            }
        }),
        _ => Value::Int(-1),
    }
}

/// `[cap0, …, capN, name_idx]` → a closure value carrying its target subroutine
/// name-index and captured values (by value — Go captures by reference, a
/// documented gap).
fn b_closure_new(vm: &mut VM, argc: u8) -> Value {
    let mut args = pop_args(vm, argc);
    let name_idx = args.pop().map(|v| v.to_int()).unwrap_or(-1);
    Value::Obj(heap_alloc(HostObj::Closure {
        name_idx,
        captures: args,
    }))
}

/// `[closure, idx]` → the closure's captured value at `idx`.
fn b_closure_get(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let idx = args.get(1).map(|v| v.to_int()).unwrap_or(0);
    match args.first() {
        Some(Value::Obj(id)) => HEAP.with(|h| {
            let h = h.borrow();
            match h.get(*id as usize) {
                Some(HostObj::Closure { captures, .. }) => usize::try_from(idx)
                    .ok()
                    .and_then(|i| captures.get(i))
                    .cloned()
                    .unwrap_or(Value::Undef),
                _ => Value::Undef,
            }
        }),
        _ => Value::Undef,
    }
}

/// `min(a, b, …)` — the smallest argument, preserving int vs float, compared
/// numerically (or lexicographically when all arguments are strings).
fn b_min(vm: &mut VM, argc: u8) -> Value {
    fold_extreme(vm, argc, true)
}

/// `max(a, b, …)` — the largest argument (see [`b_min`]).
fn b_max(vm: &mut VM, argc: u8) -> Value {
    fold_extreme(vm, argc, false)
}

fn fold_extreme(vm: &mut VM, argc: u8, want_min: bool) -> Value {
    let args = pop_args(vm, argc);
    let all_str = args.iter().all(|v| matches!(v, Value::Str(_)));
    args.into_iter()
        .reduce(|a, b| {
            let pick_b = if all_str {
                let (x, y) = (go_str(&a), go_str(&b));
                if want_min {
                    y < x
                } else {
                    y > x
                }
            } else if want_min {
                b.to_float() < a.to_float()
            } else {
                b.to_float() > a.to_float()
            };
            if pick_b {
                b
            } else {
                a
            }
        })
        .unwrap_or(Value::Undef)
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
    /// A closure: the name-index of its compiled `$lambda_N` subroutine (for
    /// dynamic dispatch when passed as a value) plus its captured values.
    Closure { name_idx: i64, captures: Vec<Value> },
    /// A one-slot mutable box for a variable captured by reference: the enclosing
    /// scope and every capturing closure share this handle, so writes propagate.
    Cell(Value),
}

thread_local! {
    /// The host-owned Go object heap. `Value::Obj(id)` indexes this slab; it
    /// grows per run and is cleared by [`heap_reset`] at the start of every
    /// program so handles never leak across runs.
    static HEAP: RefCell<Vec<HostObj>> = const { RefCell::new(Vec::new()) };

    /// A stack of defer frames, one per in-flight function invocation that has
    /// `defer` statements. Each frame holds its deferred closures in push order;
    /// the drain loop pops them LIFO before the function returns.
    static DEFERS: RefCell<Vec<Vec<Value>>> = const { RefCell::new(Vec::new()) };

    /// The value of an in-flight `panic`, or `None`. Set by `panic()`, cleared by
    /// `recover()`; the compiler unwinds through defer drains while it is `Some`.
    static PANIC: RefCell<Option<Value>> = const { RefCell::new(None) };
}

/// Clear the object heap, defer stack, and panic state. Called at each run start.
pub fn heap_reset() {
    HEAP.with(|h| h.borrow_mut().clear());
    DEFERS.with(|d| d.borrow_mut().clear());
    PANIC.with(|p| *p.borrow_mut() = None);
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
            // Go prints a function value as a hex pointer; a fixed marker suffices.
            Some(HostObj::Closure { .. }) => "<func>".to_string(),
            // A cell is an internal box; render its contents (a captured value).
            Some(HostObj::Cell(v)) => go_str(v),
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
    let chars: Vec<char> = fmt.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '%' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        i += 1;
        if i >= chars.len() {
            out.push('%');
            break;
        }
        // flags
        let (mut left, mut zero, mut plus) = (false, false, false);
        while i < chars.len() {
            match chars[i] {
                '-' => left = true,
                '0' => zero = true,
                '+' => plus = true,
                ' ' | '#' => {}
                _ => break,
            }
            i += 1;
        }
        // width
        let mut width = 0usize;
        let mut has_width = false;
        while i < chars.len() && chars[i].is_ascii_digit() {
            has_width = true;
            width = width * 10 + (chars[i] as usize - '0' as usize);
            i += 1;
        }
        // precision
        let mut prec: Option<usize> = None;
        if i < chars.len() && chars[i] == '.' {
            i += 1;
            let mut p = 0usize;
            while i < chars.len() && chars[i].is_ascii_digit() {
                p = p * 10 + (chars[i] as usize - '0' as usize);
                i += 1;
            }
            prec = Some(p);
        }
        if i >= chars.len() {
            break;
        }
        let verb = chars[i];
        i += 1;

        // Render the argument per verb (nil-safe on a missing argument).
        let body = match verb {
            '%' => {
                out.push('%');
                continue;
            }
            't' => rest.next().map(go_str).unwrap_or_default(),
            'q' => format!("\"{}\"", rest.next().map(go_str).unwrap_or_default()),
            'f' | 'F' => {
                let v = rest.next().map(|v| v.to_float()).unwrap_or(0.0);
                let s = format!("{:.*}", prec.unwrap_or(6), v);
                if plus && v >= 0.0 {
                    format!("+{s}")
                } else {
                    s
                }
            }
            'd' => {
                let n = rest.next().map(|v| v.to_int()).unwrap_or(0);
                if plus && n >= 0 {
                    format!("+{n}")
                } else {
                    n.to_string()
                }
            }
            'x' => format!("{:x}", rest.next().map(|v| v.to_int()).unwrap_or(0)),
            'X' => format!("{:X}", rest.next().map(|v| v.to_int()).unwrap_or(0)),
            'o' => format!("{:o}", rest.next().map(|v| v.to_int()).unwrap_or(0)),
            'b' => format!("{:b}", rest.next().map(|v| v.to_int()).unwrap_or(0)),
            'c' => char::from_u32(rest.next().map(|v| v.to_int()).unwrap_or(0) as u32)
                .map(|c| c.to_string())
                .unwrap_or_default(),
            // %v, %s and anything else: Go's `%v` rendering, with precision
            // truncating a string.
            _ => {
                let mut s = rest.next().map(go_str).unwrap_or_default();
                if let Some(p) = prec {
                    if s.chars().count() > p {
                        s = s.chars().take(p).collect();
                    }
                }
                s
            }
        };

        // Apply width padding (right-justified by default; `-` left, `0` zero-fill
        // for numeric verbs, after any sign).
        let body_len = body.chars().count();
        if has_width && body_len < width {
            let pad = width - body_len;
            if left {
                out.push_str(&body);
                out.push_str(&" ".repeat(pad));
            } else if zero && matches!(verb, 'd' | 'f' | 'F' | 'x' | 'X' | 'o' | 'b') {
                let (sign, digits) = match body.strip_prefix(['-', '+']) {
                    Some(d) => (&body[..1], d),
                    None => ("", body.as_str()),
                };
                out.push_str(sign);
                out.push_str(&"0".repeat(pad));
                out.push_str(digits);
            } else {
                out.push_str(&" ".repeat(pad));
                out.push_str(&body);
            }
        } else {
            out.push_str(&body);
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
    pub const COUNT: u16 = 842;
    pub const TRIM_PREFIX: u16 = 843;
    pub const TRIM_SUFFIX: u16 = 844;
    pub const TRIM: u16 = 845;
    pub const TITLE: u16 = 846;
    pub const EQUAL_FOLD: u16 = 847;
    pub const LAST_INDEX: u16 = 848;
    pub const REPLACE: u16 = 849;
    // strconv.*
    pub const ITOA: u16 = 850;
    pub const ATOI: u16 = 851;
    pub const PARSE_INT: u16 = 852;
    pub const PARSE_FLOAT: u16 = 853;
    pub const FORMAT_INT: u16 = 854;
    pub const QUOTE: u16 = 855;
    // math.*
    pub const ABS: u16 = 860;
    pub const SQRT: u16 = 861;
    pub const POW: u16 = 862;
    pub const FLOOR: u16 = 863;
    pub const CEIL: u16 = 864;
    pub const ROUND: u16 = 865;
    pub const TRUNC: u16 = 866;
    pub const MOD_F: u16 = 867;
    pub const HYPOT: u16 = 868;
    pub const MAX_F: u16 = 869;
    pub const MIN_F: u16 = 870;
    // sort.*
    pub const SORT_INTS: u16 = 875;
    pub const SORT_STRINGS: u16 = 876;
    pub const SORT_FLOAT64S: u16 = 877;
    // os.*
    pub const GETENV: u16 = 880;

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
            ("strings", "Replace") => REPLACE,
            ("strings", "ReplaceAll") => REPLACE_ALL,
            ("strings", "Fields") => FIELDS,
            ("strings", "Count") => COUNT,
            ("strings", "TrimPrefix") => TRIM_PREFIX,
            ("strings", "TrimSuffix") => TRIM_SUFFIX,
            ("strings", "Trim") => TRIM,
            ("strings", "Title") => TITLE,
            ("strings", "EqualFold") => EQUAL_FOLD,
            ("strings", "LastIndex") => LAST_INDEX,
            ("strconv", "Itoa") => ITOA,
            ("strconv", "Atoi") => ATOI,
            ("strconv", "ParseInt") => PARSE_INT,
            ("strconv", "ParseFloat") => PARSE_FLOAT,
            ("strconv", "FormatInt") => FORMAT_INT,
            ("strconv", "Quote") => QUOTE,
            ("math", "Abs") => ABS,
            ("math", "Sqrt") => SQRT,
            ("math", "Pow") => POW,
            ("math", "Floor") => FLOOR,
            ("math", "Ceil") => CEIL,
            ("math", "Round") => ROUND,
            ("math", "Trunc") => TRUNC,
            ("math", "Mod") => MOD_F,
            ("math", "Hypot") => HYPOT,
            ("math", "Max") => MAX_F,
            ("math", "Min") => MIN_F,
            ("sort", "Ints") => SORT_INTS,
            ("sort", "Strings") => SORT_STRINGS,
            ("sort", "Float64s") => SORT_FLOAT64S,
            ("os", "Getenv") => GETENV,
            _ => return None,
        })
    }

    /// Resolve a package constant `pkg.NAME` (e.g. `math.Pi`) to its value, or
    /// `None` if unknown. Used by the compiler for bare selector values.
    pub fn resolve_const(pkg: &str, name: &str) -> Option<Value> {
        Some(match (pkg, name) {
            ("math", "Pi") => Value::Float(std::f64::consts::PI),
            ("math", "E") => Value::Float(std::f64::consts::E),
            ("math", "Sqrt2") => Value::Float(std::f64::consts::SQRT_2),
            ("math", "MaxInt64") => Value::Int(i64::MAX),
            ("math", "MinInt64") => Value::Int(i64::MIN),
            ("math", "MaxInt") => Value::Int(i64::MAX),
            ("math", "MinInt") => Value::Int(i64::MIN),
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
        vm.register_builtin(REPLACE, b_replace);
        vm.register_builtin(REPLACE_ALL, b_replace_all);
        vm.register_builtin(SPLIT, b_split);
        vm.register_builtin(FIELDS, b_fields);
        vm.register_builtin(JOIN, b_join);
        vm.register_builtin(ITOA, b_itoa);
        vm.register_builtin(ATOI, b_atoi);
        // extra strings.*
        vm.register_builtin(COUNT, b_count);
        vm.register_builtin(TRIM_PREFIX, |vm, a| {
            two_str(vm, a, |s, p| s.strip_prefix(p).unwrap_or(s).to_string())
        });
        vm.register_builtin(TRIM_SUFFIX, |vm, a| {
            two_str(vm, a, |s, p| s.strip_suffix(p).unwrap_or(s).to_string())
        });
        vm.register_builtin(TRIM, |vm, a| {
            two_str(vm, a, |s, cut| {
                s.trim_matches(|c| cut.contains(c)).to_string()
            })
        });
        vm.register_builtin(TITLE, |vm, a| s1(vm, a, title_case));
        vm.register_builtin(EQUAL_FOLD, |vm, a| {
            b2(vm, a, |s, t| s.eq_ignore_ascii_case(t))
        });
        vm.register_builtin(LAST_INDEX, b_last_index);
        // extra strconv.*
        vm.register_builtin(PARSE_INT, b_parse_int);
        vm.register_builtin(PARSE_FLOAT, |vm, a| {
            let args = pop_args(vm, a);
            Value::Float(
                args.first()
                    .map(go_str)
                    .unwrap_or_default()
                    .trim()
                    .parse()
                    .unwrap_or(0.0),
            )
        });
        vm.register_builtin(FORMAT_INT, b_format_int);
        vm.register_builtin(QUOTE, |vm, a| s1(vm, a, |s| format!("\"{s}\"")));
        // math.*
        vm.register_builtin(ABS, |vm, a| math1(vm, a, f64::abs));
        vm.register_builtin(SQRT, |vm, a| math1(vm, a, f64::sqrt));
        vm.register_builtin(FLOOR, |vm, a| math1(vm, a, f64::floor));
        vm.register_builtin(CEIL, |vm, a| math1(vm, a, f64::ceil));
        vm.register_builtin(ROUND, |vm, a| math1(vm, a, f64::round));
        vm.register_builtin(TRUNC, |vm, a| math1(vm, a, f64::trunc));
        vm.register_builtin(POW, |vm, a| math2(vm, a, f64::powf));
        vm.register_builtin(MOD_F, |vm, a| math2(vm, a, |x, y| x % y));
        vm.register_builtin(HYPOT, |vm, a| math2(vm, a, f64::hypot));
        vm.register_builtin(MAX_F, |vm, a| math2(vm, a, f64::max));
        vm.register_builtin(MIN_F, |vm, a| math2(vm, a, f64::min));
        // sort.*
        vm.register_builtin(SORT_INTS, |vm, a| {
            sort_slice(vm, a, |x, y| x.to_int().cmp(&y.to_int()))
        });
        vm.register_builtin(SORT_FLOAT64S, |vm, a| {
            sort_slice(vm, a, |x, y| {
                x.to_float()
                    .partial_cmp(&y.to_float())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        });
        vm.register_builtin(SORT_STRINGS, |vm, a| {
            sort_slice(vm, a, |x, y| go_str(x).cmp(&go_str(y)))
        });
        // os.*
        vm.register_builtin(GETENV, |vm, a| {
            let args = pop_args(vm, a);
            let k = args.first().map(go_str).unwrap_or_default();
            Value::str(std::env::var(&k).unwrap_or_default())
        });
    }

    /// A two-string-arg → string builtin.
    fn two_str(vm: &mut VM, argc: u8, f: impl Fn(&str, &str) -> String) -> Value {
        let args = pop_args(vm, argc);
        let s = args.first().map(go_str).unwrap_or_default();
        let p = args.get(1).map(go_str).unwrap_or_default();
        Value::str(f(&s, &p))
    }

    /// A one-float-arg → float `math` builtin.
    fn math1(vm: &mut VM, argc: u8, f: impl Fn(f64) -> f64) -> Value {
        let args = pop_args(vm, argc);
        Value::Float(f(args.first().map(|v| v.to_float()).unwrap_or(0.0)))
    }

    /// A two-float-arg → float `math` builtin.
    fn math2(vm: &mut VM, argc: u8, f: impl Fn(f64, f64) -> f64) -> Value {
        let args = pop_args(vm, argc);
        let a = args.first().map(|v| v.to_float()).unwrap_or(0.0);
        let b = args.get(1).map(|v| v.to_float()).unwrap_or(0.0);
        Value::Float(f(a, b))
    }

    /// Sort a heap slice in place by `cmp`. Returns nil (sort.* are void).
    fn sort_slice(
        vm: &mut VM,
        argc: u8,
        cmp: impl Fn(&Value, &Value) -> std::cmp::Ordering,
    ) -> Value {
        let args = pop_args(vm, argc);
        if let Some(Value::Obj(id)) = args.first() {
            HEAP.with(|h| {
                let mut h = h.borrow_mut();
                if let Some(HostObj::Slice(a)) = h.get_mut(*id as usize) {
                    a.sort_by(cmp);
                }
            });
        }
        Value::Undef
    }

    fn title_case(s: &str) -> String {
        s.split(' ')
            .map(|w| {
                let mut c = w.chars();
                match c.next() {
                    Some(f) => f.to_uppercase().chain(c).collect::<String>(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn b_count(vm: &mut VM, argc: u8) -> Value {
        let args = pop_args(vm, argc);
        let s = args.first().map(go_str).unwrap_or_default();
        let sub = args.get(1).map(go_str).unwrap_or_default();
        Value::Int(if sub.is_empty() {
            s.chars().count() as i64 + 1
        } else {
            s.matches(&sub).count() as i64
        })
    }

    fn b_last_index(vm: &mut VM, argc: u8) -> Value {
        let args = pop_args(vm, argc);
        let s = args.first().map(go_str).unwrap_or_default();
        let sub = args.get(1).map(go_str).unwrap_or_default();
        Value::Int(s.rfind(&sub).map(|b| b as i64).unwrap_or(-1))
    }

    fn b_parse_int(vm: &mut VM, argc: u8) -> Value {
        let args = pop_args(vm, argc);
        let s = args.first().map(go_str).unwrap_or_default();
        let base = args.get(1).map(|v| v.to_int()).unwrap_or(10).max(2) as u32;
        Value::Int(i64::from_str_radix(s.trim(), base).unwrap_or(0))
    }

    fn b_format_int(vm: &mut VM, argc: u8) -> Value {
        let args = pop_args(vm, argc);
        let n = args.first().map(|v| v.to_int()).unwrap_or(0);
        let base = args.get(1).map(|v| v.to_int()).unwrap_or(10);
        Value::str(match base {
            2 => format!("{n:b}"),
            8 => format!("{n:o}"),
            16 => format!("{n:x}"),
            _ => n.to_string(),
        })
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

    fn b_replace(vm: &mut VM, argc: u8) -> Value {
        let args = pop_args(vm, argc);
        let s = args.first().map(go_str).unwrap_or_default();
        let old = args.get(1).map(go_str).unwrap_or_default();
        let new = args.get(2).map(go_str).unwrap_or_default();
        let n = args.get(3).map(|v| v.to_int()).unwrap_or(-1);
        if old.is_empty() {
            return Value::str(s);
        }
        Value::str(if n < 0 {
            s.replace(&old, &new)
        } else {
            s.replacen(&old, &new, n as usize)
        })
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
        // The zero value of an erased generic type parameter (`var total T`) is
        // nil (`Undef`); Go would use T's concrete zero. Treat nil as the
        // additive identity so a generic accumulator matches Go for whichever
        // concrete type is passed: nil+int→int, nil+float→float, nil+str→str.
        NumOp::Add if matches!(a, Value::Undef) => Ok(b.clone()),
        NumOp::Add if matches!(b, Value::Undef) => Ok(a.clone()),
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
