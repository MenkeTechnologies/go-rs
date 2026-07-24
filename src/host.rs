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
/// Enable recoverable runtime faults (emitted at program start when `recover` is
/// used).
pub const GSET_PANIC_MODE: u16 = 897;
/// Integer `a / b` with a divide-by-zero panic: stack `[a, b]`.
pub const GIDIV: u16 = 898;
/// Integer `a % b` with a divide-by-zero panic: stack `[a, b]`.
pub const GIMOD: u16 = 899;
/// A Go type conversion `T(v)`: stack `[value, typeNameConstIdx]` — the compiler
/// pushes the value then a constant naming the target type.
pub const GCONV: u16 = 900;
/// `[value]` → the runtime type name of a value (`int`/`string`/`bool`/`float64`,
/// a struct type name, `[]`/`map`/`func`, or `nil`) for type switches/assertions.
pub const GTYPETAG: u16 = 901;
/// `[value, "tag"]` → a single-result type assertion `x.(T)`: `value` if its type
/// matches, else a recoverable panic (`interface conversion`).
pub const GASSERT: u16 = 902;
/// `[iter, key]` → the loop value for `for key := range iter`. A string decodes
/// the rune (code point) starting at byte offset `key` (Go ranges strings by
/// rune); a slice/map indexes normally.
pub const GRANGE_VAL: u16 = 903;
/// `[dst, src]` → `copy(dst, src)`: copy `min(len(dst), len(src))` elements from
/// `src` (a slice, or a string for `copy([]byte, s)`) into `dst`, returning the
/// count. Writes through `dst`'s backing so a sub-slice destination aliases.
pub const GCOPY: u16 = 904;

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
    vm.register_builtin(GSET_PANIC_MODE, b_set_panic_mode);
    vm.register_builtin(GIDIV, b_idiv);
    vm.register_builtin(GIMOD, b_imod);
    vm.register_builtin(GCONV, b_conv);
    vm.register_builtin(GTYPETAG, b_typetag);
    vm.register_builtin(GASSERT, b_assert);
    vm.register_builtin(GRANGE_VAL, b_range_val);
    vm.register_builtin(GCOPY, b_copy);
    stdlib::install(vm);
}

/// `[value, "tag"]` → a single-result type assertion. Returns the value when its
/// runtime type matches; otherwise a recoverable panic. `tag` empty (an
/// interface type like `any`) always matches.
fn b_assert(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let v = args.first().cloned().unwrap_or(Value::Undef);
    let want = args.get(1).map(go_str).unwrap_or_default();
    let got = type_tag_of(&v);
    if want.is_empty() || want == got {
        v
    } else {
        runtime_panic(
            vm,
            format!("interface conversion: interface {{}} is {got}, not {want}"),
        );
        Value::Undef
    }
}

/// The runtime type tag of a value (shared by [`b_typetag`] and [`b_assert`]).
fn type_tag_of(v: &Value) -> String {
    match v {
        Value::Int(_) => "int".to_string(),
        Value::Float(_) => "float64".to_string(),
        Value::Str(_) => "string".to_string(),
        Value::Bool(_) => "bool".to_string(),
        Value::Obj(id) => HEAP.with(|h| match h.borrow().get(*id as usize) {
            Some(HostObj::Struct { type_name, .. }) => type_name.clone(),
            Some(HostObj::Slice(_)) | Some(HostObj::SliceView { .. }) => "[]".to_string(),
            Some(HostObj::Map(_)) => "map".to_string(),
            Some(HostObj::Closure { .. }) => "func".to_string(),
            _ => "nil".to_string(),
        }),
        _ => "nil".to_string(),
    }
}

/// `[value]` → the runtime type name used by type switches and assertions.
fn b_typetag(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    Value::str(type_tag_of(args.first().unwrap_or(&Value::Undef)))
}

/// `[value, "type"]` → a Go type conversion `T(value)`. Integer types truncate
/// and wrap to their width; float types widen/narrow; `string(n)` is the UTF-8
/// encoding of code point `n`, and `string([]byte/[]rune)` joins the elements.
fn b_conv(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let v = args.first().cloned().unwrap_or(Value::Undef);
    let ty = args.get(1).map(go_str).unwrap_or_default();
    match ty.as_str() {
        "int" | "int64" | "uint" | "uint64" | "uintptr" => Value::Int(to_int_wide(&v)),
        "int8" => Value::Int(to_int_wide(&v) as i8 as i64),
        "int16" => Value::Int(to_int_wide(&v) as i16 as i64),
        "int32" | "rune" => Value::Int(to_int_wide(&v) as i32 as i64),
        "uint8" | "byte" => Value::Int(to_int_wide(&v) as u8 as i64),
        "uint16" => Value::Int(to_int_wide(&v) as u16 as i64),
        "uint32" => Value::Int(to_int_wide(&v) as u32 as i64),
        "float32" => Value::Float(v.to_float() as f32 as f64),
        "float64" => Value::Float(v.to_float()),
        "bool" => Value::bool(v.is_truthy()),
        "string" => conv_string(&v),
        // `[]byte(s)` — the string's UTF-8 bytes as a slice of ints.
        "[]byte" => match &v {
            Value::Str(s) => {
                let elems = s.bytes().map(|b| Value::Int(b as i64)).collect();
                Value::Obj(heap_alloc(HostObj::Slice(elems)))
            }
            _ => v,
        },
        // `[]rune(s)` — the string's Unicode code points as a slice of ints.
        "[]rune" => match &v {
            Value::Str(s) => {
                let elems = s.chars().map(|c| Value::Int(c as i64)).collect();
                Value::Obj(heap_alloc(HostObj::Slice(elems)))
            }
            _ => v,
        },
        // An unknown/named type conversion is the identity (dynamic value model).
        _ => v,
    }
}

/// The integer value of `v` for a conversion (a float truncates toward zero).
fn to_int_wide(v: &Value) -> i64 {
    match v {
        Value::Float(f) => *f as i64,
        other => other.to_int(),
    }
}

/// `string(v)`: a code point becomes its UTF-8 char; a `[]byte`/`[]rune` slice
/// joins its elements; a string is unchanged.
fn conv_string(v: &Value) -> Value {
    match v {
        Value::Str(_) => v.clone(),
        Value::Int(n) => {
            let s = char::from_u32(*n as u32)
                .map(|c| c.to_string())
                .unwrap_or_else(|| "\u{FFFD}".to_string());
            Value::str(s)
        }
        Value::Obj(id) => {
            if let Some((_, _, len)) = slice_backing(*id) {
                let elems: Vec<i64> = (0..len)
                    .filter_map(|i| slice_get(*id, i))
                    .map(|e| e.to_int())
                    .collect();
                // go-rs erases the slice element type, so `string(slice)` must
                // disambiguate `[]byte` (UTF-8 bytes to decode) from `[]rune`
                // (code points to join). If every element is a byte and they form
                // a valid multibyte UTF-8 sequence, decode as bytes; otherwise
                // join as code points. This makes `string([]byte(s)) == s` for a
                // real string while a lone code point ≥ 128 (not valid standalone
                // UTF-8) still joins as a rune.
                let all_bytes = elems.iter().all(|&e| (0..=255).contains(&e));
                let has_high = elems.iter().any(|&e| e >= 128);
                if all_bytes && has_high {
                    let bytes: Vec<u8> = elems.iter().map(|&e| e as u8).collect();
                    if let Ok(s) = std::str::from_utf8(&bytes) {
                        return Value::str(s.to_string());
                    }
                }
                let s: String = elems
                    .iter()
                    .filter_map(|&e| char::from_u32(e as u32))
                    .collect();
                Value::str(s)
            } else {
                v.clone()
            }
        }
        _ => Value::str(go_str(v)),
    }
}

/// `[a, b]` → `a / b` (integer), panicking on divide-by-zero like Go.
fn b_idiv(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let a = args.first().map(Value::to_int).unwrap_or(0);
    let b = args.get(1).map(Value::to_int).unwrap_or(0);
    if b == 0 {
        runtime_panic(vm, "integer divide by zero");
        return Value::Int(0);
    }
    Value::Int(a.wrapping_div(b))
}

/// `[a, b]` → `a % b` (integer), panicking on divide-by-zero like Go.
fn b_imod(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let a = args.first().map(Value::to_int).unwrap_or(0);
    let b = args.get(1).map(Value::to_int).unwrap_or(0);
    if b == 0 {
        runtime_panic(vm, "integer divide by zero");
        return Value::Int(0);
    }
    Value::Int(a.wrapping_rem(b))
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
        // A sub-slice shares the parent's backing array (so element writes are
        // visible both ways), matching Go — collapse a view-of-a-view to the
        // original backing.
        Value::Obj(id) => {
            let Some((backing, base, len)) = slice_backing(id) else {
                return Value::Undef;
            };
            let len = len as i64;
            let lo = if lo_raw < 0 { 0 } else { lo_raw }.clamp(0, len) as usize;
            let hi = if hi_raw < 0 { len } else { hi_raw }.clamp(0, len) as usize;
            let hi = hi.max(lo);
            Value::Obj(heap_alloc(HostObj::SliceView {
                backing,
                offset: base + lo,
                len: hi - lo,
            }))
        }
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
        // Go ranges a string by rune: the keys are the byte offsets where each
        // rune starts (its index in `range`), not every byte offset.
        Some(Value::Str(s)) => s
            .char_indices()
            .map(|(i, _)| Value::Int(i as i64))
            .collect(),
        Some(Value::Obj(id)) => {
            if let Some((_, _, len)) = slice_backing(*id) {
                (0..len as i64).map(Value::Int).collect()
            } else {
                HEAP.with(|h| match h.borrow().get(*id as usize) {
                    Some(HostObj::Map(m)) => m.iter().map(|(k, _)| k.clone()).collect(),
                    _ => Vec::new(),
                })
            }
        }
        _ => Vec::new(),
    };
    Value::Obj(heap_alloc(HostObj::Slice(keys)))
}

/// `[iter, key]` → the loop value for `for key := range iter`. A string yields
/// the rune (code point) starting at byte offset `key`; anything else indexes
/// by the key (slice element or map value).
fn b_range_val(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let iter = args.first().cloned().unwrap_or(Value::Undef);
    let key = args.get(1).cloned().unwrap_or(Value::Undef);
    match &iter {
        Value::Str(s) => {
            let off = key.to_int().max(0) as usize;
            match s.get(off..).and_then(|rest| rest.chars().next()) {
                Some(c) => Value::Int(c as i64),
                None => Value::Int(0),
            }
        }
        _ => {
            // Slice/map: index normally.
            vm.push(iter);
            vm.push(key);
            b_index_get(vm, 2)
        }
    }
}

// ── host-owned heap for Go composite types ─────────────────────────────────
//
// `Value::Obj(id)` is an opaque handle into [`HEAP`]; slices and maps are Go
// reference types, so sharing a handle is exactly right. Structs are value
// types — the compiler emits a `GSTRUCT_COPY` on assignment / parameter bind /
// return so a struct handle is never aliased (Go copy semantics).

/// One object on the host-owned Go heap.
pub(crate) enum HostObj {
    /// A slice that owns its backing array. Go slices are reference types.
    Slice(Vec<Value>),
    /// A sub-slice view `s[lo:hi]` sharing another slice's backing array at an
    /// offset, so element writes are visible through the parent (and vice versa).
    /// `backing` indexes a [`HostObj::Slice`]; `cap` is `backing.len() - offset`.
    SliceView {
        backing: u32,
        offset: usize,
        len: usize,
    },
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
    PANIC_MODE.with(|m| *m.borrow_mut() = false);
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

/// Resolve a slice handle to `(backing slice id, offset, len)`. A plain
/// [`HostObj::Slice`] is its own backing at offset 0; a [`HostObj::SliceView`]
/// names the backing it shares. `None` if `id` is not a slice.
fn slice_backing(id: u32) -> Option<(u32, usize, usize)> {
    HEAP.with(|h| match h.borrow().get(id as usize) {
        Some(HostObj::Slice(a)) => Some((id, 0, a.len())),
        Some(HostObj::SliceView {
            backing,
            offset,
            len,
        }) => Some((*backing, *offset, *len)),
        _ => None,
    })
}

/// Read element `i` of a slice-or-view (bounds-checked against its length).
fn slice_get(id: u32, i: usize) -> Option<Value> {
    let (backing, offset, len) = slice_backing(id)?;
    if i >= len {
        return None;
    }
    HEAP.with(|h| match h.borrow().get(backing as usize) {
        Some(HostObj::Slice(a)) => a.get(offset + i).cloned(),
        _ => None,
    })
}

/// Write element `i` of a slice-or-view (bounds-checked), through its backing so
/// a sub-slice write is visible to the parent. Returns whether it landed.
fn slice_set(id: u32, i: usize, v: Value) -> bool {
    let Some((backing, offset, len)) = slice_backing(id) else {
        return false;
    };
    if i >= len {
        return false;
    }
    HEAP.with(|h| {
        if let Some(HostObj::Slice(a)) = h.borrow_mut().get_mut(backing as usize) {
            if let Some(slot) = a.get_mut(offset + i) {
                *slot = v;
                return true;
            }
        }
        false
    })
}

/// `copy(dst, src)` — copy `min(len(dst), len(src))` elements into `dst`,
/// returning the count. `src` may be a slice or a string (`copy([]byte, s)`).
fn b_copy(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let Some(Value::Obj(dst)) = args.first().cloned() else {
        return Value::Int(0);
    };
    let dst_len = slice_backing(dst).map(|(_, _, l)| l).unwrap_or(0);
    let src_vals: Vec<Value> = match args.get(1) {
        Some(Value::Obj(sid)) => {
            let slen = slice_backing(*sid).map(|(_, _, l)| l).unwrap_or(0);
            (0..slen).filter_map(|i| slice_get(*sid, i)).collect()
        }
        Some(Value::Str(s)) => s.bytes().map(|b| Value::Int(b as i64)).collect(),
        _ => return Value::Int(0),
    };
    let n = dst_len.min(src_vals.len());
    for (i, v) in src_vals.into_iter().take(n).enumerate() {
        slice_set(dst, i, v);
    }
    Value::Int(n as i64)
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
            let len = s.len();
            return match usize::try_from(i).ok().and_then(|i| s.as_bytes().get(i)) {
                Some(b) => Value::Int(*b as i64),
                None => {
                    runtime_panic(vm, format!("index out of range [{i}] with length {len}"));
                    Value::Undef
                }
            };
        }
        _ => {
            ffi_fault(vm, "go-rs: invalid index of nil".to_string());
            return Value::Undef;
        }
    };
    // A slice or sub-slice view: index into its backing (bounds-checked).
    if let Some((_, _, len)) = slice_backing(id) {
        let i = key.to_int();
        return match usize::try_from(i).ok().filter(|&i| i < len) {
            Some(i) => slice_get(id, i).unwrap_or(Value::Undef),
            None => {
                runtime_panic(vm, format!("index out of range [{i}] with length {len}"));
                Value::Undef
            }
        };
    }
    HEAP.with(|h| {
        let h = h.borrow();
        match h.get(id as usize) {
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
    // A slice or sub-slice view: write through the backing array at `offset+i`.
    if let Some((backing, offset, len)) = slice_backing(id) {
        let i = key.to_int();
        let err = match usize::try_from(i).ok().filter(|&i| i < len) {
            Some(i) => HEAP.with(|h| {
                if let Some(HostObj::Slice(a)) = h.borrow_mut().get_mut(backing as usize) {
                    a[offset + i] = val.clone();
                }
                None::<String>
            }),
            None => Some(format!("index out of range [{i}] with length {len}")),
        };
        return match err {
            None => val,
            Some(msg) => {
                runtime_panic(vm, msg);
                Value::Undef
            }
        };
    }
    let err = HEAP.with(|h| {
        let mut h = h.borrow_mut();
        match h.get_mut(id as usize) {
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
            // A Go runtime fault (index OOB) is recoverable; an internal type
            // error keeps its `go-rs:` prefix and aborts.
            if msg.starts_with("go-rs:") {
                ffi_fault(vm, msg);
            } else {
                runtime_panic(vm, msg);
            }
            Value::Undef
        }
    }
}

/// `len(x)` — slice/map element count or string byte length.
fn b_len(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    match args.first() {
        Some(Value::Str(s)) => Value::Int(s.len() as i64),
        Some(Value::Obj(id)) => {
            if let Some((_, _, len)) = slice_backing(*id) {
                return Value::Int(len as i64);
            }
            HEAP.with(|h| match h.borrow().get(*id as usize) {
                Some(HostObj::Map(m)) => Value::Int(m.len() as i64),
                _ => Value::Int(0),
            })
        }
        _ => Value::Int(0),
    }
}

/// `cap(x)` — a slice's capacity: `backing.len() - offset` (a sub-slice can grow
/// into the remaining backing without reallocating, like Go).
fn b_cap(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    match args.first() {
        Some(Value::Str(s)) => Value::Int(s.len() as i64),
        Some(Value::Obj(id)) => {
            if let Some((backing, offset, _)) = slice_backing(*id) {
                let cap = HEAP.with(|h| match h.borrow().get(backing as usize) {
                    Some(HostObj::Slice(a)) => a.len().saturating_sub(offset),
                    _ => 0,
                });
                return Value::Int(cap as i64);
            }
            Value::Int(0)
        }
        _ => Value::Int(0),
    }
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
            // Appending to a sub-slice view reallocates into a fresh slice (its
            // own backing), so it never clobbers the parent's elements.
            let is_view = HEAP
                .with(|h| matches!(h.borrow().get(id as usize), Some(HostObj::SliceView { .. })));
            if is_view {
                let (backing, offset, len) = match slice_backing(id) {
                    Some(t) => t,
                    None => return Value::Obj(heap_alloc(HostObj::Slice(args))),
                };
                let new_len = len + args.len();
                // Go semantics: if the shared backing has spare capacity after the
                // view, the new elements are written *in place* (clobbering the
                // parent's data there); only a full backing forces a reallocation
                // into a fresh, independent slice.
                let cap = HEAP.with(|h| match h.borrow().get(backing as usize) {
                    Some(HostObj::Slice(a)) => a.len().saturating_sub(offset),
                    _ => 0,
                });
                if new_len <= cap {
                    HEAP.with(|h| {
                        if let Some(HostObj::Slice(a)) = h.borrow_mut().get_mut(backing as usize) {
                            for (k, v) in args.into_iter().enumerate() {
                                a[offset + len + k] = v;
                            }
                        }
                    });
                    return Value::Obj(heap_alloc(HostObj::SliceView {
                        backing,
                        offset,
                        len: new_len,
                    }));
                }
                let mut out = HEAP.with(|h| match h.borrow().get(backing as usize) {
                    Some(HostObj::Slice(a)) => a[offset..offset + len].to_vec(),
                    _ => Vec::new(),
                });
                out.extend(args);
                return Value::Obj(heap_alloc(HostObj::Slice(out)));
            }
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
            runtime_panic(vm, "invalid memory address or nil pointer dereference");
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
            runtime_panic(vm, "invalid memory address or nil pointer dereference");
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

    /// Whether the running program uses `recover`, so a runtime fault (divide by
    /// zero, index out of range, nil dereference) should become a *recoverable*
    /// panic (set `PANIC` and let the compiler's unwind machinery run) rather than
    /// aborting the VM. Set once at program start by `GSET_PANIC_MODE`.
    static PANIC_MODE: RefCell<bool> = const { RefCell::new(false) };
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

/// A Go runtime fault. When the program uses `recover` (panic mode), record it as
/// a catchable panic whose value is `runtime error: <msg>` (what Go's `recover()`
/// yields) and let the compiler-emitted unwind checks handle it. Otherwise abort
/// the run with a terse `go-rs:` diagnostic, as before.
fn runtime_panic(vm: &mut VM, msg: impl Into<String>) {
    let full = format!("runtime error: {}", msg.into());
    if PANIC_MODE.with(|m| *m.borrow()) {
        // Recoverable: record it and let the compiler's unwind checks run.
        PANIC.with(|p| *p.borrow_mut() = Some(Value::str(full)));
    } else {
        // Unrecovered: print like Go's first line and exit 2. Halt the VM too so
        // no further ops run before the process exits (stdout is flushed first).
        use std::io::Write as _;
        let _ = std::io::stdout().flush();
        eprintln!("panic: {full}");
        vm.request_halt();
        std::process::exit(2);
    }
}

/// `GSET_PANIC_MODE`: enable recoverable runtime faults (the program uses
/// `recover`). Emitted once at program start.
fn b_set_panic_mode(_vm: &mut VM, _argc: u8) -> Value {
    PANIC_MODE.with(|m| *m.borrow_mut() = true);
    Value::Undef
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
            Some(HostObj::SliceView {
                backing,
                offset,
                len,
            }) => {
                let parts: Vec<String> = match h.get(*backing as usize) {
                    Some(HostObj::Slice(a)) => {
                        a[*offset..*offset + *len].iter().map(go_str).collect()
                    }
                    _ => Vec::new(),
                };
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
            // Sort the backing in place over the (sub-)slice's element range, so
            // sorting a view (`sort.Ints(s[1:4])`) sorts through the parent.
            if let Some((backing, offset, len)) = super::slice_backing(*id) {
                HEAP.with(|h| {
                    if let Some(HostObj::Slice(a)) = h.borrow_mut().get_mut(backing as usize) {
                        a[offset..offset + len].sort_by(cmp);
                    }
                });
            }
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
                let elems: &[Value] = match h.get(*id as usize) {
                    Some(HostObj::Slice(a)) => a,
                    Some(HostObj::SliceView {
                        backing,
                        offset,
                        len,
                    }) => match h.get(*backing as usize) {
                        Some(HostObj::Slice(a)) => &a[*offset..*offset + *len],
                        _ => &[],
                    },
                    _ => &[],
                };
                elems.iter().map(go_str).collect::<Vec<_>>().join(&sep)
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
