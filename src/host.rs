//! Host builtins and the strict numeric hook for go-rs.
//!
//! fusevm runs the lowered chunk; this module supplies the runtime behavior the
//! bytecode can't express directly: the `fmt` print family (`Println`/`Print`/
//! `Printf`) and the Go builtins `println`/`print`, plus a [`numeric_hook`] that
//! gives `+` its string-concatenation overload and `<`/`==`/… their string
//! ordering, wraps overflowing integer arithmetic, and decides nil and mixed
//! `int`/`float64` identity. Values render with Go's `fmt` `%v` rules
//! ([`go_str`]).

use fusevm::{NumOp, Value, VM};
use std::cell::RefCell;
use std::collections::HashMap;

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
/// Park the in-flight panic for the duration of one deferred call. Emitted by
/// the drain loop immediately before invoking a deferred closure.
///
/// Go runs a deferred function *normally* even while a panic is propagating: it
/// may call other functions, and only a `recover()` the deferred function makes
/// **itself** stops the panic. Leaving the panic flagged would make the unwind
/// check after each of those inner calls throw the deferred function out before
/// it reaches its `recover()`. Parking also records the call depth, which is
/// what makes the "directly" rule enforceable.
pub const GDEFER_PARK: u16 = 961;
/// Restore a parked panic after its deferred call returns, unless that call
/// recovered it (or panicked afresh, which supersedes).
pub const GDEFER_UNPARK: u16 = 962;

/// `[received, zero]` → `zero` when `received` is the drained-closed-channel
/// sentinel, else `received` itself. Every receive goes through this, so the
/// sentinel never escapes into a Go value.
pub const GCHAN_VAL: u16 = 963;
/// `[received]` → `Bool`: the `ok` of `v, ok := <-ch`. False exactly when the
/// receive found the channel closed and drained.
pub const GCHAN_OK: u16 = 964;
/// `[array, elemType]` → a copy of a fixed-size array value (Go array value
/// semantics on assign / pass / return / container read / container store /
/// channel send / `append` / `range`).
///
/// The element type is passed because an array and a slice are the same heap
/// object here, so only the written type says whether an element is itself a
/// value (copy) or a reference (share).
pub const GARRAY_COPY: u16 = 965;
/// `[container, item…]` → the same container with the items appended.
///
/// fusevm's `Op::CallBuiltin` carries its argument count in a `u8`, so one call
/// can take at most 255 stack values. A composite literal is not bounded by
/// that — `[]int{…}` with 256 elements is ordinary Go — so a literal longer
/// than one call can carry is built in chunks: the first chunk goes to
/// [`GSLICE_LIT`] / [`GMAP_LIT`] / [`GSTRUCT_NEW`] and each later chunk to this,
/// which dispatches on what it is handed (slice elements, `k,v` map pairs, or
/// `name,value` struct fields). Before this existed the count wrapped and the
/// literal silently lost every element past the first 255.
pub const GLIT_EXTEND: u16 = 966;
/// `[array, type]` → the same object, tagged as the fixed-size array type it
/// was written as, so `%T` and `%#v` can name it.
///
/// A `[N]T` and a `[]T` are the same [`HostObj::Slice`], and the length is not
/// recoverable from the elements (`[3]int` and a 3-element `[]int` hold the
/// same thing), so the *written* type is stamped on the object where an array
/// value is born — a composite literal and a zero value. Every other array is a
/// copy of one of those, and [`GARRAY_COPY`] carries the tag across, so the tag
/// survives assignment, a parameter bind, a return and an `any` box alike.
pub const GARRAY_TAG: u16 = 967;
/// `[value, type]` → the value with every slice inside it stamped with its
/// *written* element type, for a `fmt` argument position only.
///
/// `fmt` reads `[]byte` as text under `%s`, `%q` and `%x` but distributes those
/// verbs over the elements of any other slice — `%q` of a `[]byte("ab")` is
/// `"ab"` and of a `[]int{97, 98}` is `['a' 'b']`. Nothing in the values tells
/// the two apart: both are integer elements. So the written type rides in from
/// the compiler, which walks it alongside the value and tags each slice node
/// (including the ones nested in an array, a slice or a map value). Like the
/// [`GF32_BOX`] / [`GU64_BOX`] width tags, the tagged value is a rebuilt copy
/// used for display only, so the program's own slice is untouched — and an
/// operand whose static type the compiler does not know is left untagged, where
/// the byte-slice guess from the element values stands in.
pub const GELEM_TAG: u16 = 968;
/// `[value, type]` → the value tagged with the defined type it was written as,
/// for a `fmt` argument position only.
///
/// `type Weekday int` declares a type distinct from `int` that is represented
/// exactly like one, so nothing at run time separates a `Weekday` from the `int`
/// holding the same number — but `%T` prints `main.Weekday` and `%#v` writes the
/// name too. The compiler knows the static type at the call site and tags the
/// operand here, the same way the `float32` and `uint64` width tags ride in.
/// Every verb but `%T` and `%#v` sees straight through it to the value.
pub const GNAMED_BOX: u16 = 969;
/// `[a, b, ne]` → Go's `==` (or `!=` when `ne` is 1) on two **interface**
/// operands: equal when the dynamic types match *and* the values do.
///
/// Go decides interface equality by dynamic type before value, so an `int` and a
/// `float64` are never equal however the numbers line up, and neither are two
/// struct types with identical fields. Nothing else in valid Go puts two
/// different types under one operator — arithmetic and ordered comparison on
/// mismatched types are compile errors, and an interface is unordered — so this
/// is the whole of the rule's reach.
///
/// It cannot be left to the native op or to [`numeric_hook`]. fusevm answers an
/// `Int`/`Float` pair natively by promoting the integer, so the pair never
/// reaches the frontend at all; and the pairs that *do* reach the hook land on
/// its rendered-string fallback, where `any(1) == any("1")` compares `"1"` with
/// `"1"` and says yes. So the compiler routes a comparison with an
/// interface-typed operand here instead of emitting `Op::NumEq`/`Op::StrEq`.
///
/// The dynamic type is [`go_type_name`] — the same function `%T` prints, which
/// already separates a struct type from another with the same fields, and a
/// typed nil (`HostObj::Nil`) from an untyped one. It does **not** separate two
/// integer widths: `int`, `int64` and `uint` are all `Value::Int` and all name
/// `int`, so `any(1) == any(int64(1))` is still wrong (BUGS.md).
pub const GIFACE_EQ: u16 = 970;
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
/// `[map, key]` → the comma-ok map lookup `v, ok := m[key]` as a 2-element slice
/// `[value, present]`: the value (or zero) and whether the key was present.
pub const GMAP_GET2: u16 = 905;
/// `[base, xs]` → `append(base, xs...)`: a new slice of `base`'s elements
/// followed by every element of the spread slice `xs`.
pub const GAPPEND_SPREAD: u16 = 906;

/// `[a, b]` → `a / b` chosen at run time from the operand representations.
///
/// Go's `/` is integer division when both operands are integers and float
/// division otherwise, and the choice is made statically by the type checker.
/// go-rs's compiler infers a numeric category for most expressions, but some
/// forms (indexing a slice/map, a method result, an `interface{}` value) come
/// back [`NumType::Unknown`]; emitting the float `Op::Div` for those made
/// `xs[0] / 2` yield `3.5` where Go yields `3`. This builtin resolves the same
/// rule against the actual values, so an untyped-at-compile-time integer pair
/// still truncates toward zero (and panics on a zero divisor) like Go.
///
/// Numbered above the `stdlib`/`math` block (which runs to 921) — 907 collides
/// with `math.Sin`.
pub const GDYNDIV: u16 = 950;

/// `[value]` → the same handle, marked as a Go pointer (`&T{…}` / `new(T)`) so
/// `==` compares it by address rather than field by field.
pub const GPTR_MARK: u16 = 955;
/// `[typeName, "m1,m2,…"]` — record a concrete type's method set. Emitted once
/// per method-bearing type in the program prologue, and only when the program
/// tests a value against an interface's method set.
pub const GREG_METHODS: u16 = 952;
/// `[value, "m1,m2,…"]` → `Bool`: whether the value's dynamic type implements
/// every named method — Go's interface satisfaction, which is what a type
/// assertion or type-switch case against an interface type tests.
pub const GIFACE_OK: u16 = 953;
/// `[value, "m1,m2,…", "<interface display>"]` → the value when its dynamic type
/// implements the method set, else a recoverable `interface conversion` panic
/// naming the first missing method (Go's message).
pub const GASSERT_IFACE: u16 = 954;

/// `[a, b]` → IEEE float `a / b`: `±Inf` for a nonzero numerator over zero and
/// `NaN` for `0.0 / 0.0`, as Go's float division does.
///
/// fusevm's native `Op::Div` yields `Undef` for a zero divisor (it has no
/// float-specific division), so `1.0 / z` printed `<nil>`. The compiler still
/// emits `Op::Div` — which the JIT and AOT keep in registers — whenever the
/// divisor is a provably-nonzero constant, and falls back here otherwise.
pub const GFDIV: u16 = 951;

/// `[value]` → the same number tagged `float32` (`HostObj::F32`) for `fmt`, or
/// a fresh slice of tagged elements when the operand is a `[]float32`. Emitted
/// only at `fmt` argument positions.
pub const GF32_BOX: u16 = 957;

/// `[lhs, rhs, op]` → one arithmetic operation performed at 32-bit width, `op`
/// being an [`f32_op`] code.
///
/// Rounding an `f64` result to `f32` afterwards is a *different* computation:
/// the double rounding can land a ulp away. `float32(16777217) * float32(0.2)`
/// is `3.3554432e+06` rounded once and `3.3554434e+06` rounded twice. So a
/// `float32` operation is done in `f32` throughout, which is why it costs a
/// builtin rather than a native op.
pub const GF32_ARITH: u16 = 958;

/// The [`GF32_ARITH`] operator codes, shared with the compiler.
pub mod f32_op {
    pub const ADD: i64 = 0;
    pub const SUB: i64 = 1;
    pub const MUL: i64 = 2;
    pub const DIV: i64 = 3;
}

/// `[value]` → the same number tagged `uint64` (`HostObj::U64`) for `fmt`, so it
/// renders as an unsigned 64-bit integer. Emitted only at `fmt` argument
/// positions, and — like [`GF32_BOX`] — element-wise through a slice or map.
///
/// `uint64` / `uint` / `uintptr` share `Value::Int`'s 64-bit two's-complement
/// representation, so `+ - * << & | ^ &^` need nothing: the bit pattern is
/// already Go's answer. Only the operations that *read* the sign differ, and
/// display is one of them — `uint64(0) - 1` holds `-1i64` and must print
/// `18446744073709551615`.
pub const GU64_BOX: u16 = 959;

/// `[lhs, rhs, op]` → one operation performed at unsigned 64-bit width, `op`
/// being a [`u64_op`] code: the operations whose result depends on the sign bit
/// (`/`, `%`, `>>`, the four ordered comparisons, and the conversion to
/// `float64`).
pub const GU64_ARITH: u16 = 960;

/// The [`GU64_ARITH`] operator codes, shared with the compiler.
pub mod u64_op {
    pub const DIV: i64 = 0;
    pub const MOD: i64 = 1;
    pub const SHR: i64 = 2;
    pub const LT: i64 = 3;
    pub const LE: i64 = 4;
    pub const GT: i64 = 5;
    pub const GE: i64 = 6;
    /// Unary: `float64(u)` — the unsigned value widened, not the signed one.
    pub const TOFLOAT: i64 = 7;
}

/// `["[]T" | "map[K]V"]` → the typed nil that is that type's zero value
/// (`HostObj::Nil`). Emitted wherever the compiler needs a slice's or map's
/// zero value, so `fmt` can print `[]` / `map[]` rather than `<nil>`.
pub const GNIL_OF: u16 = 956;

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
    vm.register_builtin(GARRAY_COPY, b_array_copy);
    vm.register_builtin(GLIT_EXTEND, b_lit_extend);
    vm.register_builtin(GARRAY_TAG, b_array_tag);
    vm.register_builtin(GELEM_TAG, b_elem_tag);
    vm.register_builtin(GNAMED_BOX, b_named_box);
    vm.register_builtin(GIFACE_EQ, b_iface_eq);
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
    vm.register_builtin(GDEFER_PARK, b_defer_park);
    vm.register_builtin(GDEFER_UNPARK, b_defer_unpark);
    vm.register_builtin(GCHAN_VAL, b_chan_val);
    vm.register_builtin(GCHAN_OK, b_chan_ok);
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
    vm.register_builtin(GMAP_GET2, b_map_get2);
    vm.register_builtin(GAPPEND_SPREAD, b_append_spread);
    vm.register_builtin(GDYNDIV, b_dyndiv);
    vm.register_builtin(GFDIV, b_fdiv);
    vm.register_builtin(GPTR_MARK, b_ptr_mark);
    vm.register_builtin(GNIL_OF, b_nil_of);
    vm.register_builtin(GF32_BOX, b_f32_box);
    vm.register_builtin(GF32_ARITH, b_f32_arith);
    vm.register_builtin(GU64_BOX, b_u64_box);
    vm.register_builtin(GU64_ARITH, b_u64_arith);
    vm.register_builtin(GREG_METHODS, b_reg_methods);
    vm.register_builtin(GIFACE_OK, b_iface_ok);
    vm.register_builtin(GASSERT_IFACE, b_assert_iface);
    stdlib::install(vm);
}

thread_local! {
    /// Every concrete type's method set, keyed by the tag [`type_tag_of`]
    /// produces. Populated by [`GREG_METHODS`] in the program prologue; the
    /// interface tests read it to decide satisfaction.
    static METHOD_SETS: RefCell<std::collections::HashMap<String, Vec<String>>> =
        RefCell::new(std::collections::HashMap::new());
}

/// `[typeName, "m1,m2,…"]` — record a concrete type's method set.
fn b_reg_methods(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let ty = args.first().map(go_str).unwrap_or_default();
    let methods: Vec<String> = args
        .get(1)
        .map(go_str)
        .unwrap_or_default()
        .split(',')
        .filter(|m| !m.is_empty())
        .map(str::to_string)
        .collect();
    METHOD_SETS.with(|m| m.borrow_mut().insert(ty, methods));
    Value::Undef
}

/// The first method of `want` that `v`'s dynamic type does not have, or `None`
/// when it implements them all. A nil value has no methods, so it satisfies only
/// the empty interface — as in Go, where a nil interface fails every assertion.
fn missing_method(v: &Value, want: &str) -> Option<String> {
    let tag = type_tag_of(v);
    METHOD_SETS.with(|m| {
        let m = m.borrow();
        let have = m.get(&tag);
        want.split(',')
            .filter(|w| !w.is_empty())
            .find(|w| !have.is_some_and(|h| h.iter().any(|x| x == w)))
            .map(str::to_string)
    })
}

/// `[value, "m1,m2,…"]` → whether the value's dynamic type implements them all.
fn b_iface_ok(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let v = args.first().cloned().unwrap_or(Value::Undef);
    let want = args.get(1).map(go_str).unwrap_or_default();
    Value::bool(missing_method(&v, &want).is_none())
}

/// `[value, "m1,m2,…", display]` → the value, or an `interface conversion` panic.
fn b_assert_iface(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let v = args.first().cloned().unwrap_or(Value::Undef);
    let want = args.get(1).map(go_str).unwrap_or_default();
    let display = args.get(2).map(go_str).unwrap_or_default();
    match missing_method(&v, &want) {
        None => v,
        Some(m) => {
            let got = go_type_name(&v);
            runtime_panic(
                vm,
                format!("interface conversion: {got} is not {display}: missing method {m}"),
            );
            Value::Undef
        }
    }
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
            Some(HostObj::Slice { .. }) | Some(HostObj::SliceView { .. }) => "[]".to_string(),
            Some(HostObj::Map(_)) => "map".to_string(),
            // A typed nil keeps its type's tag, so a type switch on a nil slice
            // still picks the `[]T` case rather than the nil default.
            Some(HostObj::Nil { kind, .. }) => match kind {
                NilKind::Slice => "[]".to_string(),
                NilKind::Map => "map".to_string(),
            },
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
                Value::Obj(heap_alloc(HostObj::slice(elems)))
            }
            _ => v,
        },
        // `[]rune(s)` — the string's Unicode code points as a slice of ints.
        "[]rune" => match &v {
            Value::Str(s) => {
                let elems = s.chars().map(|c| Value::Int(c as i64)).collect();
                Value::Obj(heap_alloc(HostObj::slice(elems)))
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

/// `[a, b]` → `a / b`, picking Go's integer or float division from the operand
/// representations. Both integers ⇒ truncating integer division (panicking on a
/// zero divisor); anything else ⇒ float division, where `x / 0.0` is ±Inf.
fn b_dyndiv(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let a = args.first().cloned().unwrap_or(Value::Undef);
    let b = args.get(1).cloned().unwrap_or(Value::Undef);
    match (&a, &b) {
        (Value::Int(x), Value::Int(y)) => {
            if *y == 0 {
                runtime_panic(vm, "integer divide by zero");
                return Value::Int(0);
            }
            Value::Int(x.wrapping_div(*y))
        }
        _ => Value::Float(a.to_float() / b.to_float()),
    }
}

/// `[a, b]` → IEEE float `a / b` (`±Inf` / `NaN` on a zero divisor).
fn b_fdiv(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let a = args.first().map(Value::to_float).unwrap_or(0.0);
    let b = args.get(1).map(Value::to_float).unwrap_or(0.0);
    Value::Float(a / b)
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

/// `s[low:high:max]` on a slice, or `s[low:high]` on a slice or string: stack
/// `[recv, low, high, max]`. Returns a view sharing the parent's backing array
/// (or a substring). A bound of `-1` means "omitted": `0` / `len` / `cap`.
fn b_slice_sub(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let recv = args.first().cloned().unwrap_or(Value::Undef);
    let lo_raw = args.get(1).map(Value::to_int).unwrap_or(-1);
    let hi_raw = args.get(2).map(Value::to_int).unwrap_or(-1);
    let max_raw = args.get(3).map(Value::to_int).unwrap_or(-1);
    match recv {
        // A sub-slice shares the parent's backing array (so element writes are
        // visible both ways), matching Go — collapse a view-of-a-view to the
        // original backing.
        Value::Obj(id) => {
            let Some((backing, base, len)) = slice_backing(id) else {
                return Value::Undef;
            };
            // Go bounds a re-slice by capacity, not length: `s[:cap(s)]` is
            // legal and exposes the backing array's spare room, so an element an
            // append wrote past `len` is reachable through `s[0:len+1]`.
            let cap = slice_cap(id).unwrap_or(len) as i64;
            let len = len as i64;
            let lo = if lo_raw < 0 { 0 } else { lo_raw }.clamp(0, cap) as usize;
            let hi = if hi_raw < 0 { len } else { hi_raw }.clamp(0, cap) as usize;
            let hi = hi.max(lo);
            // The three-index form `s[lo:hi:max]` gives the result capacity
            // `max - lo`; omitted, it keeps the rest of the parent's capacity.
            let mx = if max_raw < 0 { cap } else { max_raw }.clamp(0, cap) as usize;
            let mx = mx.max(hi);
            Value::Obj(heap_alloc(HostObj::SliceView {
                backing,
                offset: base + lo,
                len: hi - lo,
                cap: mx - lo,
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
fn b_recover(vm: &mut VM, _argc: u8) -> Value {
    // Go: `recover()` is only effective when the function calling it was itself
    // invoked directly as a deferred call of a panicking frame. The drain loop
    // parks the panic at the draining frame's depth, so the deferred function's
    // own body runs exactly [`DEFERRED_BODY_DEPTH`] frames deeper; anything it
    // calls in turn is deeper still and gets nil.
    PARKED.with(|p| {
        let mut parked = p.borrow_mut();
        let Some(top) = parked.last_mut() else {
            return Value::Undef;
        };
        if top.recovered || vm.frames.len() != top.depth + DEFERRED_BODY_DEPTH {
            return Value::Undef;
        }
        match top.val.take() {
            Some(v) => {
                top.recovered = true;
                v
            }
            None => Value::Undef,
        }
    })
}

/// [`GDEFER_PARK`] — take the in-flight panic out of circulation for one
/// deferred call, remembering the depth its `recover()` must be called from.
fn b_defer_park(vm: &mut VM, _argc: u8) -> Value {
    let val = PANIC.with(|p| p.borrow_mut().take());
    PARKED.with(|p| {
        p.borrow_mut().push(Park {
            val,
            depth: vm.frames.len(),
            recovered: false,
        })
    });
    Value::Undef
}

/// The value a receive on a **closed and drained** channel yields, installed on
/// the scheduler with `Scheduler::with_recv_zero`.
///
/// `Scheduler::recv` returns this — and only this — for the one case Go reports
/// as `ok == false`, so it is the exact signal `v, ok := <-ch` and
/// `for v := range ch` need, decided atomically inside the scheduler rather than
/// by a racy "is it closed *and* empty?" check afterwards. It is a fresh heap
/// handle, and `Value::Obj` is identity-comparable, so no Go value can collide
/// with it — unlike the default `Value::Int(0)`, which a channel carrying a real
/// `0` produces all the time.
pub fn chan_closed_sentinel() -> Value {
    CHAN_CLOSED.with(|c| {
        c.borrow_mut()
            .get_or_insert_with(|| Value::Obj(heap_alloc(HostObj::ChanClosed)))
            .clone()
    })
}

/// Whether `v` is the drained-closed-channel sentinel.
fn is_chan_closed(v: &Value) -> bool {
    let Value::Obj(id) = v else { return false };
    HEAP.with(|h| matches!(h.borrow().get(*id as usize), Some(HostObj::ChanClosed)))
}

/// [`GCHAN_VAL`] — a receive's value: the element type's zero when the channel
/// was closed and drained, else what the channel delivered.
fn b_chan_val(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let v = args.first().cloned().unwrap_or(Value::Undef);
    if is_chan_closed(&v) {
        return args.get(1).cloned().unwrap_or(Value::Undef);
    }
    v
}

/// [`GCHAN_OK`] — a receive's `ok`.
fn b_chan_ok(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    Value::bool(!args.first().map(is_chan_closed).unwrap_or(false))
}

/// [`GDEFER_UNPARK`] — the deferred call has returned. Resume the panic it was
/// running under unless that call recovered it, or unless the call raised a
/// panic of its own, which Go lets supersede the one it was deferred for.
fn b_defer_unpark(_vm: &mut VM, _argc: u8) -> Value {
    let Some(park) = PARKED.with(|p| p.borrow_mut().pop()) else {
        return Value::Undef;
    };
    if let Some(v) = park.val {
        PANIC.with(|p| {
            let mut slot = p.borrow_mut();
            if slot.is_none() {
                *slot = Some(v);
            }
        });
    }
    Value::Undef
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
        // Go 1.22's range-over-int: `for i := range n` yields 0 … n-1. A
        // non-positive `n` yields nothing, which is why this is a range and not
        // an error. Without this arm an integer produced *no* keys, so the loop
        // silently ran zero times.
        Some(Value::Int(n)) => (0..*n).map(Value::Int).collect(),
        _ => Vec::new(),
    };
    Value::Obj(heap_alloc(HostObj::slice(keys)))
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
        // Range-over-int has only one loop variable — the integer itself — so
        // the "value" is the key. Go rejects a second variable there, which the
        // parser reports rather than inventing one.
        Value::Int(_) => key,
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
    ///
    /// `arr_ty` is `Some` when the object is a fixed-size **array** rather than
    /// a slice, and holds the type as written (`[3]int`, `[2][3]int`). The two
    /// are the same object here — a `[N]T` is a value and a `[]T` a reference,
    /// which the *static* type drives at every copy site — but `%T` and `%#v`
    /// read the run-time value, and nothing in the elements distinguishes a
    /// `[3]int` from a 3-element `[]int`. The tag is set where an array is born
    /// (a composite literal, a zero value) and carried by [`array_copy`], so it
    /// survives into an `any` where a `fmt`-position box could not.
    ///
    /// `elem_ty` is the written element type (`byte`, `int`, `string`, …) and is
    /// set only by [`b_elem_tag`], on the display copy of a `fmt` argument. It
    /// exists because `fmt` reads a `[]byte` as text under `%s`/`%q`/`%x` and
    /// distributes those verbs over the elements of every other slice, and the
    /// elements alone cannot tell a `[]byte` from a `[]int`. `None` means the
    /// compiler had no static type for the operand, and the formatter falls back
    /// to guessing from the element values.
    Slice {
        elems: Vec<Value>,
        arr_ty: Option<String>,
        elem_ty: Option<String>,
    },
    /// A sub-slice view `s[lo:hi]` sharing another slice's backing array at an
    /// offset, so element writes are visible through the parent (and vice versa).
    /// `backing` indexes a [`HostObj::Slice`]. `cap` is the view's own capacity:
    /// normally `backing.len() - offset`, but the three-index form `s[lo:hi:max]`
    /// records the smaller `max - lo` so an append past it reallocates instead of
    /// writing into backing the view no longer owns.
    SliceView {
        backing: u32,
        offset: usize,
        len: usize,
        cap: usize,
    },
    /// A map, insertion-ordered for stable iteration; keys compared by value.
    Map(Vec<(Value, Value)>),
    /// A struct: its type name and ordered `(field, value)` pairs.
    ///
    /// `by_ref` marks a handle produced by `&T{…}` / `new(T)` — a Go *pointer*
    /// rather than a struct value. go-rs models both as the same heap handle, but
    /// `==` does not treat them alike: Go compares struct values field by field
    /// and pointers by address. A `by_ref` handle therefore compares by identity,
    /// which is what makes two `errors.New("x")` values distinct (and so what
    /// `errors.Is` walks a wrap chain looking for). A [`GSTRUCT_COPY`] of one
    /// clears the flag: the copy is a value, not the pointer.
    Struct {
        type_name: String,
        fields: Vec<(String, Value)>,
        by_ref: bool,
    },
    /// A closure: the name-index of its compiled `$lambda_N` subroutine (for
    /// dynamic dispatch when passed as a value) plus its captured values.
    Closure { name_idx: i64, captures: Vec<Value> },
    /// A one-slot mutable box for a variable captured by reference: the enclosing
    /// scope and every capturing closure share this handle, so writes propagate.
    Cell(Value),
    /// A `float32` at a `fmt` argument position. Go's `fmt` renders a float with
    /// the shortest decimal that round-trips **at the value's own width**, so a
    /// `float32` and the `float64` holding the same bits print differently
    /// (`0.33333334` vs `0.3333333333333333`). The value model has one float
    /// width, so the compiler boxes a statically-`float32` operand here on its
    /// way into `fmt` — and nowhere else, which keeps the box out of arithmetic.
    F32(f32),
    /// A `uint64` / `uint` / `uintptr` at a `fmt` argument position. The value
    /// model holds one integer width (`Value::Int`, an `i64`) whose bit pattern
    /// is already right for unsigned arithmetic; only *rendering* reads the sign
    /// bit, so the compiler boxes a statically-unsigned operand on its way into
    /// `fmt` — and nowhere else, which keeps the box out of arithmetic. `ty` is
    /// the written type so `%T` names it exactly.
    U64 { val: u64, ty: String },
    /// The unique marker a receive on a closed, drained channel yields — see
    /// [`chan_closed_sentinel`]. It is never stored in a Go variable: every
    /// receive site maps it to the element type's zero immediately.
    ChanClosed,
    /// The zero value of a slice or map type — Go's *typed* nil. It is not
    /// `Value::Undef`, because Go distinguishes a nil slice (`[]`, `len` 0,
    /// appendable) and a nil map (`map[]`, readable, not writable) from a nil
    /// interface (`<nil>`), and prints all three differently. `ty` is the written
    /// type, so `%T` and `%#v` name it exactly.
    ///
    /// It still compares equal to `nil`: [`numeric_hook`] answers `Eq`/`Ne`
    /// against [`Value::Undef`] for these handles, so `s == nil` stays true.
    Nil { kind: NilKind, ty: String },
    /// A value at a `fmt` argument position whose static type was a defined one
    /// (`type Weekday int`). A defined type is represented exactly like its
    /// base, so this box holds the base value untouched and adds only the name
    /// `%T` and `%#v` print. [`b_named_box`] applies it; the formatter unwraps
    /// it everywhere else, and it never reaches the program.
    Named { ty: String, inner: Value },
}

impl HostObj {
    /// A plain slice — the untagged [`HostObj::Slice`] every slice-producing
    /// path wants. An array is this plus a tag, which [`b_array_tag`] stamps on
    /// where the array is born and [`array_copy`] carries across.
    fn slice(elems: Vec<Value>) -> HostObj {
        HostObj::Slice {
            elems,
            arr_ty: None,
            elem_ty: None,
        }
    }
}

/// Which kind of typed nil a [`HostObj::Nil`] is — the two Go composite types
/// whose zero value is usable rather than a fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NilKind {
    Slice,
    Map,
}

thread_local! {
    /// One [`HostObj::Nil`] handle per written type. Nil slices and maps are
    /// immutable (an `append` reallocates, a map write panics), so every
    /// `var s []int` can share a handle; memoizing keeps a loop that declares one
    /// per iteration from growing the heap. Cleared by [`heap_reset`].
    static NILS: RefCell<std::collections::HashMap<String, Value>> =
        RefCell::new(std::collections::HashMap::new());
}

/// `[typeName]` → the typed nil for that slice or map type.
fn b_nil_of(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let ty = args.first().map(go_str).unwrap_or_default();
    NILS.with(|n| {
        n.borrow_mut()
            .entry(ty.clone())
            .or_insert_with(|| {
                let kind = if ty.starts_with("map[") {
                    NilKind::Map
                } else {
                    NilKind::Slice
                };
                Value::Obj(heap_alloc(HostObj::Nil { kind, ty }))
            })
            .clone()
    })
}

/// [`GF32_BOX`] — tag the `float32`s in a `fmt` argument. Stack `[value, spec]`:
/// an empty `spec` tags the value itself, otherwise `spec` names the struct
/// fields to tag (`"x,y"`). Either way a slice or map operand is handled
/// element-wise, so `[]float32`, `map[string]float32` and `[]point` all work.
fn b_f32_box(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let v = args.first().cloned().unwrap_or(Value::Undef);
    let spec = args.get(1).map(go_str).unwrap_or_default();
    box_for_fmt(&v, &spec, BoxTag::F32)
}

/// [`GU64_BOX`] — tag the unsigned 64-bit integers in a `fmt` argument. Stack
/// `[value, spec, ty]`, with `spec` read exactly as [`b_f32_box`] reads it.
fn b_u64_box(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let v = args.first().cloned().unwrap_or(Value::Undef);
    let spec = args.get(1).map(go_str).unwrap_or_default();
    let ty = args
        .get(2)
        .map(go_str)
        .unwrap_or_else(|| "uint64".to_string());
    box_for_fmt(&v, &spec, BoxTag::U64(&ty))
}

/// Which width tag [`box_for_fmt`] applies to the leaves it reaches.
#[derive(Clone, Copy)]
enum BoxTag<'a> {
    F32,
    U64(&'a str),
}

/// Apply `tag` to one leaf value.
fn box_leaf(v: &Value, tag: BoxTag) -> Value {
    match tag {
        BoxTag::F32 => box_f32(v),
        BoxTag::U64(ty) => box_u64(v, ty),
    }
}

/// One value tagged as an unsigned 64-bit integer. A non-integer passes through
/// untouched, so the box is safe wherever the static type says `uint64`.
fn box_u64(v: &Value, ty: &str) -> Value {
    match v {
        Value::Int(n) => Value::Obj(heap_alloc(HostObj::U64 {
            val: *n as u64,
            ty: ty.to_string(),
        })),
        other => other.clone(),
    }
}

/// The `u64` a value carries when it is a [`HostObj::U64`] box.
fn unbox_u64(v: &Value) -> Option<u64> {
    let Value::Obj(id) = v else { return None };
    HEAP.with(|h| match h.borrow().get(*id as usize) {
        Some(HostObj::U64 { val, .. }) => Some(*val),
        _ => None,
    })
}

/// [`GU64_ARITH`] — one operation at unsigned 64-bit width. Both operands are
/// read as the `u64` their `i64` bit pattern denotes, so no caller has to have
/// boxed them.
fn b_u64_arith(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let a = args.first().map(arg_int).unwrap_or(0) as u64;
    let b = args.get(1).map(arg_int).unwrap_or(0) as u64;
    match args.get(2).map(Value::to_int).unwrap_or(u64_op::DIV) {
        // Go panics on a zero divisor whatever the signedness.
        u64_op::DIV => match a.checked_div(b) {
            Some(q) => Value::Int(q as i64),
            None => {
                runtime_panic(vm, "integer divide by zero");
                Value::Int(0)
            }
        },
        u64_op::MOD => match a.checked_rem(b) {
            Some(r) => Value::Int(r as i64),
            None => {
                runtime_panic(vm, "integer divide by zero");
                Value::Int(0)
            }
        },
        // A shift count at or past the width yields 0 in Go, where Rust's `>>`
        // would panic on overflow.
        u64_op::SHR => Value::Int(if b >= 64 { 0 } else { (a >> b) as i64 }),
        u64_op::LT => Value::bool(a < b),
        u64_op::LE => Value::bool(a <= b),
        u64_op::GT => Value::bool(a > b),
        u64_op::GE => Value::bool(a >= b),
        // Unary — `b` is unused.
        _ => Value::Float(a as f64),
    }
}

/// Tag every `float32` a `fmt` argument holds, per [`b_f32_box`]'s `spec`.
/// Composites are rebuilt rather than mutated — the tag is a display detail and
/// must not be visible to the program that owns the original.
fn box_for_fmt(v: &Value, spec: &str, tag: BoxTag) -> Value {
    if let Some(es) = slice_elems(v) {
        let elems = es.iter().map(|e| box_for_fmt(e, spec, tag)).collect();
        // The rebuild keeps the `[N]T` tag, or `%T` on an array of `float32` /
        // `uint64` would fall back to naming it a slice.
        let arr_ty = match v {
            Value::Obj(id) => HEAP.with(|h| match h.borrow().get(*id as usize) {
                Some(HostObj::Slice { arr_ty, .. }) => arr_ty.clone(),
                _ => None,
            }),
            _ => None,
        };
        return Value::Obj(heap_alloc(HostObj::Slice {
            elems,
            arr_ty,
            elem_ty: None,
        }));
    }
    if let Some(pairs) = map_pairs(v) {
        let boxed = pairs
            .into_iter()
            .map(|(k, val)| (k, box_for_fmt(&val, spec, tag)))
            .collect();
        return Value::Obj(heap_alloc(HostObj::Map(boxed)));
    }
    if spec.is_empty() {
        return box_leaf(v, tag);
    }
    let Value::Obj(id) = v else { return v.clone() };
    // Snapshot before boxing: `box_leaf` allocates, and the heap borrow is not
    // re-entrant.
    let snapshot = HEAP.with(|h| match h.borrow().get(*id as usize) {
        Some(HostObj::Struct {
            type_name,
            fields,
            by_ref,
        }) => Some((type_name.clone(), fields.clone(), *by_ref)),
        _ => None,
    });
    let Some((type_name, fields, by_ref)) = snapshot else {
        return v.clone();
    };
    let fields = fields
        .into_iter()
        .map(|(n, fv)| {
            let named = spec.split(',').any(|f| f == n);
            let fv = if named { box_leaf(&fv, tag) } else { fv };
            (n, fv)
        })
        .collect();
    Value::Obj(heap_alloc(HostObj::Struct {
        type_name,
        fields,
        by_ref,
    }))
}

/// A map value's `(key, value)` pairs, or `None` when it is not a map.
fn map_pairs(v: &Value) -> Option<Vec<(Value, Value)>> {
    let Value::Obj(id) = v else { return None };
    HEAP.with(|h| match h.borrow().get(*id as usize) {
        Some(HostObj::Map(m)) => Some(m.clone()),
        _ => None,
    })
}

/// One value tagged `float32`. A non-float (a nil, a string in an `any`) passes
/// through untouched, so the box is safe wherever the static type says `float32`.
fn box_f32(v: &Value) -> Value {
    match v {
        Value::Float(f) => Value::Obj(heap_alloc(HostObj::F32(*f as f32))),
        Value::Int(n) => Value::Obj(heap_alloc(HostObj::F32(*n as f32))),
        other => other.clone(),
    }
}

/// The `f32` a value carries when it is a [`HostObj::F32`] box.
fn unbox_f32(v: &Value) -> Option<f32> {
    let Value::Obj(id) = v else { return None };
    HEAP.with(|h| match h.borrow().get(*id as usize) {
        Some(HostObj::F32(f)) => Some(*f),
        _ => None,
    })
}

/// A `fmt` argument's numeric value, seeing through a [`HostObj::F32`] box.
fn arg_float(v: &Value) -> f64 {
    unbox_f32(v).map_or_else(|| v.to_float(), f64::from)
}

/// A `fmt` argument's integer value, seeing through a [`HostObj::F32`] or
/// [`HostObj::U64`] box. The `u64` is returned as its `i64` bit pattern, which
/// is what every unsigned operation consumes; only *rendering* re-reads it as
/// unsigned (see [`arg_uint`]).
fn arg_int(v: &Value) -> i64 {
    if let Some(u) = unbox_u64(v) {
        return u as i64;
    }
    unbox_f32(v).map_or_else(|| v.to_int(), |f| f as i64)
}

/// A `fmt` argument's value as an unsigned 64-bit integer when it is tagged one
/// — the only thing that makes `%d`/`%x`/`%o`/`%b` print the unsigned digits.
fn arg_uint(v: &Value) -> Option<u64> {
    unbox_u64(v)
}

/// [`GF32_ARITH`] — one arithmetic operation at 32-bit width.
fn b_f32_arith(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let a = args.first().map(Value::to_float).unwrap_or(0.0) as f32;
    let b = args.get(1).map(Value::to_float).unwrap_or(0.0) as f32;
    let r = match args.get(2).map(Value::to_int).unwrap_or(f32_op::ADD) {
        f32_op::SUB => a - b,
        f32_op::MUL => a * b,
        f32_op::DIV => a / b,
        _ => a + b,
    };
    Value::Float(f64::from(r))
}

/// The kind of typed nil `v` is, if it is one.
fn nil_kind(v: &Value) -> Option<NilKind> {
    let Value::Obj(id) = v else { return None };
    HEAP.with(|h| match h.borrow().get(*id as usize) {
        Some(HostObj::Nil { kind, .. }) => Some(*kind),
        _ => None,
    })
}

/// How many VM frames deeper than the draining frame a deferred function's own
/// body runs. `defer f(a)` never calls `f` from the drain loop directly: the
/// compiler snapshots the callee and arguments into a synthesized zero-argument
/// closure (see `compile_defer`) and the drain loop calls *that*, which then
/// calls `f`. So the chain is always `draining frame → snapshot closure → f`,
/// two frames, whatever `f` is — a function literal, a named function, a
/// func-valued variable, or a method value. `recover()` is Go's "direct" one
/// exactly at this depth.
const DEFERRED_BODY_DEPTH: usize = 2;

/// A panic parked for the duration of one deferred call — see [`GDEFER_PARK`].
struct Park {
    /// The panic value, taken by the `recover()` that claims it.
    val: Option<Value>,
    /// The VM call depth of the frame that is draining defers. A `recover()` is
    /// Go's "direct" one exactly when it runs at `depth + 1`.
    depth: usize,
    /// Whether a `recover()` has already claimed this panic.
    recovered: bool,
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

    /// One entry per deferred call currently running, innermost last. A drain
    /// loop parks the propagating panic here before each deferred call and
    /// restores it after, so `recover()` can be answered by frame depth.
    static PARKED: RefCell<Vec<Park>> = const { RefCell::new(Vec::new()) };

    /// The per-run drained-closed-channel sentinel handle, allocated on first
    /// use and dropped by [`heap_reset`] along with the heap slot it names.
    static CHAN_CLOSED: RefCell<Option<Value>> = const { RefCell::new(None) };

    /// Struct type name → the `(name, written type)` of each of its fields that
    /// holds a **value**: a field declared `T` where `T` is a struct type (not
    /// `*T`), or a fixed-size array `[N]T`. Written once per compile by
    /// [`set_struct_plan`], read by [`b_struct_copy`] so a copy recurses exactly
    /// as far as Go's value semantics reach and no further: a `*T` field is a
    /// pointer and must stay aliased, and slices/maps/channels are reference
    /// types whose handle the copy shares.
    ///
    /// The field's type is carried, not just its name, because an array field's
    /// copy needs its element type — the heap cannot tell `[2][3]int` (copy the
    /// inner arrays) from `[2][]int` (share the inner slices).
    ///
    /// Keyed by field *name* rather than position: a keyed composite literal
    /// (`T{B: 1, A: 2}`) may build the field vector in written order, so a
    /// positional plan would recurse into the wrong field.
    static STRUCT_PLAN: RefCell<HashMap<String, Vec<(String, String)>>> =
        RefCell::new(HashMap::new());
}

/// Record which struct fields a value-copy must recurse into. Called by the
/// compiler, which is the only place the declared field types are known — the
/// runtime sees a `Value::Obj` handle with no type information beyond the
/// struct's own name.
pub fn set_struct_plan(plan: HashMap<String, Vec<(String, String)>>) {
    STRUCT_PLAN.with(|p| *p.borrow_mut() = plan);
}

/// Clear the object heap, defer stack, and panic state. Called at each run start.
pub fn heap_reset() {
    HEAP.with(|h| h.borrow_mut().clear());
    METHOD_SETS.with(|m| m.borrow_mut().clear());
    DEFERS.with(|d| d.borrow_mut().clear());
    PANIC.with(|p| *p.borrow_mut() = None);
    PARKED.with(|p| p.borrow_mut().clear());
    CHAN_CLOSED.with(|c| *c.borrow_mut() = None);
    PANIC_MODE.with(|m| *m.borrow_mut() = false);
    NILS.with(|n| n.borrow_mut().clear());
    stdlib::sentinels_reset();
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
        Some(HostObj::Slice { elems: a, .. }) => Some((id, 0, a.len())),
        Some(HostObj::SliceView {
            backing,
            offset,
            len,
            ..
        }) => Some((*backing, *offset, *len)),
        // A nil slice is a zero-length slice for every read: `len`, `cap`,
        // `range`, `copy` and indexing all behave as Go's do.
        Some(HostObj::Nil {
            kind: NilKind::Slice,
            ..
        }) => Some((id, 0, 0)),
        _ => None,
    })
}

/// A slice handle's capacity — `len` for a slice that owns its backing, the
/// recorded `cap` for a view. `None` if `id` is not a slice.
fn slice_cap(id: u32) -> Option<usize> {
    HEAP.with(|h| match h.borrow().get(id as usize) {
        Some(HostObj::Slice { elems: a, .. }) => Some(a.len()),
        Some(HostObj::SliceView { cap, .. }) => Some(*cap),
        Some(HostObj::Nil {
            kind: NilKind::Slice,
            ..
        }) => Some(0),
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
        Some(HostObj::Slice { elems: a, .. }) => a.get(offset + i).cloned(),
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
        if let Some(HostObj::Slice { elems: a, .. }) = h.borrow_mut().get_mut(backing as usize) {
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
        // Struct (and array) keys compare by value, field-by-field — Go's rule
        // for comparable struct/array keys — not by heap identity.
        (Value::Obj(x), Value::Obj(y)) => {
            if x == y {
                return true;
            }
            HEAP.with(|h| {
                let h = h.borrow();
                match (h.get(*x as usize), h.get(*y as usize)) {
                    (
                        Some(HostObj::Struct {
                            type_name: tx,
                            fields: fx,
                            ..
                        }),
                        Some(HostObj::Struct {
                            type_name: ty,
                            fields: fy,
                            ..
                        }),
                    ) => {
                        tx == ty
                            && fx.len() == fy.len()
                            && fx.iter().zip(fy).all(|((_, va), (_, vb))| key_eq(va, vb))
                    }
                    // An *array* key compares elementwise, which is why
                    // `m[[2]int{1, 2}]` finds the entry a differently-allocated
                    // `[2]int{1, 2}` stored. Reaching a slice here is not
                    // possible in a program `go` accepts — a slice is not a
                    // comparable type and so cannot be a map key at all.
                    (
                        Some(HostObj::Slice { elems: ex, .. }),
                        Some(HostObj::Slice { elems: ey, .. }),
                    ) => ex.len() == ey.len() && ex.iter().zip(ey).all(|(a, b)| key_eq(a, b)),
                    _ => false,
                }
            })
        }
        _ => a.to_float() == b.to_float(),
    }
}

/// The index of `key` in the map at heap `id`, comparing by value. Keys are
/// snapshotted first so a struct-key `key_eq` (which itself reads the heap) never
/// re-enters an active borrow held by the caller.
fn map_find_index(id: u32, key: &Value) -> Option<usize> {
    let keys: Vec<Value> = HEAP.with(|h| match h.borrow().get(id as usize) {
        Some(HostObj::Map(m)) => m.iter().map(|(k, _)| k.clone()).collect(),
        _ => Vec::new(),
    });
    keys.iter().position(|k| key_eq(k, key))
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
            // `-1` is the compiler's marker for an omitted capacity.
            let c = args.get(3).map(|v| v.to_int()).unwrap_or(-1);
            if n < 0 {
                ffi_fault(vm, format!("go-rs: makeslice: len out of range ({n})"));
                return Value::Undef;
            }
            if c >= 0 && c < n {
                ffi_fault(vm, format!("go-rs: makeslice: cap out of range ({c})"));
                return Value::Undef;
            }
            let (len, cap) = (n as usize, if c < 0 { n as usize } else { c as usize });
            // Each element gets its *own* zero value. Cloning one `Value` would
            // share a struct (or array) zero's handle across every slot, so a
            // write to `s[0].f` would appear in every element. `value_copy` is
            // the identity on a scalar zero, so the scalar case is unchanged.
            let elem_ty = args.get(4).map(go_str).unwrap_or_default();
            let fill = |k: usize| -> Vec<Value> {
                (0..k).map(|_| value_copy(zero.clone(), &elem_ty)).collect()
            };
            if cap == len {
                return Value::Obj(heap_alloc(HostObj::slice(fill(len))));
            }
            // A `cap > len` slice is a view over a longer backing array — the
            // same shape Go's slice header has, so `cap` reports the spare room
            // and an append that fits writes into it instead of reallocating.
            let backing = heap_alloc(HostObj::slice(fill(cap)));
            Value::Obj(heap_alloc(HostObj::SliceView {
                backing,
                offset: 0,
                len,
                cap,
            }))
        }
    }
}

/// `[]T{a, b, …}` — build a slice from the popped element values.
fn b_slice_lit(vm: &mut VM, argc: u8) -> Value {
    let elems = pop_args(vm, argc);
    Value::Obj(heap_alloc(HostObj::slice(elems)))
}

/// Insert a flat `k0,v0,k1,v1,…` run into `pairs`, a duplicate key overwriting
/// in place so the map keeps its first-mention order — Go's literal rule, and
/// the same one whether the run is the whole literal or a later chunk of it.
fn map_insert_all(pairs: &mut Vec<(Value, Value)>, flat: Vec<Value>) {
    let mut it = flat.into_iter();
    while let (Some(k), Some(v)) = (it.next(), it.next()) {
        if let Some(slot) = pairs.iter_mut().find(|(ek, _)| key_eq(ek, &k)) {
            slot.1 = v;
        } else {
            pairs.push((k, v));
        }
    }
}

/// `map[K]V{k0: v0, …}` — build a map from popped `k0,v0,k1,v1,…` pairs.
fn b_map_lit(vm: &mut VM, argc: u8) -> Value {
    let flat = pop_args(vm, argc);
    let mut pairs = Vec::with_capacity(flat.len() / 2);
    map_insert_all(&mut pairs, flat);
    Value::Obj(heap_alloc(HostObj::Map(pairs)))
}

/// [`GLIT_EXTEND`] — append a further chunk of a composite literal to the
/// container the earlier chunks built, and hand the container back.
///
/// The three literal builtins each take their items as call arguments, and
/// fusevm's arity byte stops one call at 255 stack values, so a longer literal
/// arrives here in pieces. What a piece means is read off the container: slice
/// elements, `k,v` map pairs, or `name,value` struct fields.
fn b_lit_extend(vm: &mut VM, argc: u8) -> Value {
    let mut args = pop_args(vm, argc);
    if args.is_empty() {
        return Value::Undef;
    }
    let container = args.remove(0);
    let Value::Obj(id) = container else {
        return container;
    };
    // Which composite this is, read and released before anything is written:
    // a map merge calls `key_eq` and a struct field name calls `go_str`, both
    // of which read the heap themselves, and the borrow is not re-entrant.
    enum Kind {
        Slice,
        Map,
        Struct,
        Other,
    }
    let kind = HEAP.with(|h| match h.borrow().get(id as usize) {
        Some(HostObj::Slice { .. }) => Kind::Slice,
        Some(HostObj::Map(_)) => Kind::Map,
        Some(HostObj::Struct { .. }) => Kind::Struct,
        _ => Kind::Other,
    });
    match kind {
        Kind::Slice => HEAP.with(|h| {
            if let Some(HostObj::Slice { elems, .. }) = h.borrow_mut().get_mut(id as usize) {
                elems.append(&mut args);
            }
        }),
        Kind::Map => {
            let mut pairs = HEAP.with(|h| match h.borrow_mut().get_mut(id as usize) {
                Some(HostObj::Map(p)) => std::mem::take(p),
                _ => Vec::new(),
            });
            map_insert_all(&mut pairs, args);
            HEAP.with(|h| {
                if let Some(HostObj::Map(slot)) = h.borrow_mut().get_mut(id as usize) {
                    *slot = pairs;
                }
            });
        }
        Kind::Struct => {
            let mut more = Vec::with_capacity(args.len() / 2);
            let mut it = args.into_iter();
            while let (Some(name), Some(val)) = (it.next(), it.next()) {
                more.push((go_str(&name), val));
            }
            HEAP.with(|h| {
                if let Some(HostObj::Struct { fields, .. }) = h.borrow_mut().get_mut(id as usize) {
                    fields.append(&mut more);
                }
            });
        }
        Kind::Other => {}
    }
    container
}

/// [`GARRAY_TAG`] — stamp the written `[N]T` on an array object, so `%T` and
/// `%#v` name the array rather than guessing a slice from its elements.
fn b_array_tag(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let ty = args.get(1).map(go_str).unwrap_or_default();
    let val = args.first().cloned().unwrap_or(Value::Undef);
    if let Value::Obj(id) = val {
        HEAP.with(|h| {
            if let Some(HostObj::Slice { arr_ty, .. }) = h.borrow_mut().get_mut(id as usize) {
                *arr_ty = Some(ty);
            }
        });
    }
    val
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
    // A nil map reads like an empty one (every key is absent, yielding the
    // value type's zero); only writing to it is a fault.
    let is_map = HEAP.with(|h| {
        matches!(
            h.borrow().get(id as usize),
            Some(HostObj::Map(_))
                | Some(HostObj::Nil {
                    kind: NilKind::Map,
                    ..
                })
        )
    });
    if !is_map {
        ffi_fault(vm, "go-rs: invalid index target".to_string());
        return Value::Undef;
    }
    match map_find_index(id, &key) {
        Some(i) => HEAP.with(|h| match h.borrow().get(id as usize) {
            Some(HostObj::Map(m)) => m[i].1.clone(),
            _ => Value::Undef,
        }),
        // Go returns the value type's zero value for a missing key; 0 covers the
        // common numeric case (use comma-ok for the typed zero of other types).
        None => Value::Int(0),
    }
}

/// `append(base, xs...)` — a fresh slice of `base`'s elements then every element
/// of each spread slice argument. Collecting into a new backing matches Go's
/// observable result (a caller reassigns the return value).
fn b_append_spread(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    // Argument 0 is the compiler's answer to "is the element type a Go *value*
    // type, and if so which?", from the static type of the spread operand —
    // empty when it is a reference type. Deciding it here instead, by looking at
    // what the elements happen to be, would copy a `[]*T`'s pointers and share a
    // `[][2]int`'s arrays; Go does the opposite of both.
    let elem_ty = args.first().map(go_str).unwrap_or_default();
    let mut out: Vec<Value> = Vec::new();
    let mut extend_from = |v: &Value| {
        if let Value::Obj(id) = v {
            if let Some((_, _, len)) = slice_backing(*id) {
                for i in 0..len {
                    if let Some(e) = slice_get(*id, i) {
                        // `append(dst, src...)` copies each element of `src`, so a
                        // value element in the result is independent of `src`'s.
                        out.push(if elem_ty.is_empty() {
                            e
                        } else {
                            value_copy(e, &elem_ty)
                        });
                    }
                }
            }
        }
    };
    // Then the base slice (nil → empty), then the spread slices (normally
    // exactly one).
    for a in args.iter().skip(1) {
        extend_from(a);
    }
    Value::Obj(heap_alloc(HostObj::slice(out)))
}

/// `[map, key]` → `[value, present]` for the comma-ok map lookup `v, ok := m[k]`.
fn b_map_get2(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let recv = args.first().cloned().unwrap_or(Value::Undef);
    let key = args.get(1).cloned().unwrap_or(Value::Undef);
    let (val, present) = match recv {
        Value::Obj(id) => match map_find_index(id, &key) {
            Some(i) => HEAP.with(|h| match h.borrow().get(id as usize) {
                Some(HostObj::Map(m)) => (m[i].1.clone(), true),
                _ => (Value::Undef, false),
            }),
            None => (Value::Int(0), false),
        },
        _ => (Value::Undef, false),
    };
    Value::Obj(heap_alloc(HostObj::slice(vec![val, Value::bool(present)])))
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
                if let Some(HostObj::Slice { elems: a, .. }) =
                    h.borrow_mut().get_mut(backing as usize)
                {
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
    // Go's nil map is readable but not writable.
    if nil_kind(&recv) == Some(NilKind::Map) {
        plain_panic(vm, "assignment to entry in nil map".to_string());
        return Value::Undef;
    }
    // Find an existing key (by value) without holding the borrow across key_eq,
    // then insert/overwrite under a fresh mutable borrow.
    let existing = map_find_index(id, &key);
    let err = HEAP.with(|h| {
        let mut h = h.borrow_mut();
        match h.get_mut(id as usize) {
            Some(HostObj::Map(m)) => {
                match existing {
                    Some(i) => m[i].1 = val.clone(),
                    None => m.push((key.clone(), val.clone())),
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

/// `cap(x)` — a slice's capacity: normally the room left in its backing array
/// (a sub-slice can grow into it without reallocating, like Go), or the smaller
/// bound a three-index slice `s[lo:hi:max]` recorded.
fn b_cap(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    match args.first() {
        Some(Value::Str(s)) => Value::Int(s.len() as i64),
        Some(Value::Obj(id)) => Value::Int(slice_cap(*id).unwrap_or(0) as i64),
        _ => Value::Int(0),
    }
}

/// Go's `runtime.nextslicecap`: the capacity a slice grows to when an append
/// pushes its length to `new_len` past a capacity of `old_cap`.
///
/// Port of `runtime/slice.go`:
///
/// ```text
/// newcap := oldCap
/// doublecap := newcap + newcap
/// if newLen > doublecap { return newLen }
/// const threshold = 256
/// if oldCap < threshold { return doublecap }
/// for { newcap += (newcap + 3*threshold) >> 2; if newcap >= newLen { break } }
/// return newcap
/// ```
///
/// Go then rounds the byte size up to a malloc size class, which go-rs does not
/// model (it has no sized allocator), so `cap` can read low for an append that
/// grows by more than double into a non-size-class length.
fn next_slice_cap(new_len: usize, old_cap: usize) -> usize {
    let doublecap = old_cap.saturating_mul(2);
    if new_len > doublecap {
        return new_len;
    }
    const THRESHOLD: usize = 256;
    if old_cap < THRESHOLD {
        return doublecap;
    }
    let mut newcap = old_cap;
    loop {
        newcap += (newcap + 3 * THRESHOLD) >> 2;
        if newcap >= new_len {
            return newcap;
        }
    }
}

/// Grow `elems` into a fresh backing array sized by [`next_slice_cap`] and
/// return a view of the live prefix — the reallocating half of Go's
/// `growslice`. The spare room is zero-filled with the slice's own element
/// shape so later appends have somewhere to land.
fn grow_slice(elems: Vec<Value>, old_cap: usize) -> Value {
    let new_len = elems.len();
    let cap = next_slice_cap(new_len, old_cap);
    // Go zeroes the tail of the new array; go-rs has no element type here, so
    // the filler is the untyped zero. It is never readable through the returned
    // slice (indices past `len` are out of bounds) — only a later append or a
    // re-slice within `cap` can reach it, and both overwrite it first.
    let mut backing = elems;
    backing.resize(cap, Value::Int(0));
    let id = heap_alloc(HostObj::slice(backing));
    Value::Obj(heap_alloc(HostObj::SliceView {
        backing: id,
        offset: 0,
        len: new_len,
        cap,
    }))
}

/// `append(s, elems...)` — return a slice with `elems` appended, growing the
/// backing array per Go's `growslice` when the capacity is exhausted.
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
                    None => return Value::Obj(heap_alloc(HostObj::slice(args))),
                };
                let new_len = len + args.len();
                // Go semantics: if the view has spare capacity, the new elements
                // are written *in place* (clobbering the parent's data there);
                // exhausting the capacity forces a reallocation into a fresh,
                // independent slice. A three-index view's capacity stops short of
                // the backing's end, which is exactly what makes `s[a:b:b]`
                // guarantee the next append cannot clobber the parent.
                let cap = slice_cap(id).unwrap_or(0);
                if new_len <= cap {
                    HEAP.with(|h| {
                        if let Some(HostObj::Slice { elems: a, .. }) =
                            h.borrow_mut().get_mut(backing as usize)
                        {
                            for (k, v) in args.into_iter().enumerate() {
                                a[offset + len + k] = v;
                            }
                        }
                    });
                    return Value::Obj(heap_alloc(HostObj::SliceView {
                        backing,
                        offset,
                        len: new_len,
                        cap,
                    }));
                }
                let mut out = HEAP.with(|h| match h.borrow().get(backing as usize) {
                    Some(HostObj::Slice { elems: a, .. }) => a[offset..offset + len].to_vec(),
                    _ => Vec::new(),
                });
                out.extend(args);
                return grow_slice(out, cap);
            }
            // A plain slice is its own backing with `cap == len`, so every append
            // reallocates — as it does in Go, which is why `b := append(a, x)`
            // does not alias `a`. The new backing carries Go's growth headroom,
            // so the following appends land in place and `cap` doubles the way
            // Go's does.
            let existing = HEAP.with(|h| match h.borrow().get(id as usize) {
                Some(HostObj::Slice { elems: a, .. }) => Some(a.clone()),
                // `append` to a nil slice allocates, exactly as Go's does.
                Some(HostObj::Nil {
                    kind: NilKind::Slice,
                    ..
                }) => Some(Vec::new()),
                _ => None,
            });
            if let Some(mut out) = existing {
                let old_cap = out.len();
                out.extend(args);
                return grow_slice(out, old_cap);
            }
            {
                ffi_fault(
                    vm,
                    "go-rs: first argument to append must be a slice".to_string(),
                );
                Value::Undef
            }
        }
        _ => Value::Obj(heap_alloc(HostObj::slice(args))),
    }
}

/// `delete(m, k)` — remove key `k` from map `m` (no-op if absent).
fn b_delete(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let recv = args.first().cloned().unwrap_or(Value::Undef);
    let key = args.get(1).cloned().unwrap_or(Value::Undef);
    if let Value::Obj(id) = recv {
        // Locate the key (by value) before taking the mutable borrow, so a
        // struct-key `key_eq` doesn't re-enter the heap.
        if let Some(i) = map_find_index(id, &key) {
            HEAP.with(|h| {
                if let Some(HostObj::Map(m)) = h.borrow_mut().get_mut(id as usize) {
                    m.remove(i);
                }
            });
        }
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
    Value::Obj(heap_alloc(HostObj::Struct {
        type_name,
        fields,
        by_ref: false,
    }))
}

/// A host-built error value: the same `&$errorString{s: …}` shape `fmt.Errorf`
/// builds and `errors.New` returns, so `fmt` renders it through the synthesized
/// `Error()` method and `err != nil` and `err == err` behave like any other Go
/// error. `strconv`'s `ErrSyntax` / `ErrRange` sentinels are these; the
/// `*strconv.NumError` wrapping them is [`stdlib::num_error`].
pub(crate) fn make_error(msg: String) -> Value {
    Value::Obj(heap_alloc(HostObj::Struct {
        type_name: "$errorString".to_string(),
        fields: vec![("s".to_string(), Value::str(msg))],
        by_ref: true,
    }))
}

/// `[value]` → the same handle, marked as a pointer. Emitted for `&T{…}` and
/// `new(T)`, whose results Go compares by address rather than field by field.
fn b_ptr_mark(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let v = args.first().cloned().unwrap_or(Value::Undef);
    if let Value::Obj(id) = v {
        HEAP.with(|h| {
            if let Some(HostObj::Struct { by_ref, .. }) = h.borrow_mut().get_mut(id as usize) {
                *by_ref = true;
            }
        });
    }
    v
}

/// Whether `v` is a handle Go would compare by address (a `&T{…}` pointer).
fn is_ptr(v: &Value) -> bool {
    let Value::Obj(id) = v else { return false };
    HEAP.with(|h| {
        matches!(
            h.borrow().get(*id as usize),
            Some(HostObj::Struct { by_ref: true, .. })
        )
    })
}

/// Go's `==` on two values go-rs holds as heap handles. A pointer compares by
/// address — here, by handle — so two separately allocated errors with the same
/// message are distinct; anything else falls back to the caller's structural
/// comparison. `None` when neither side is a pointer.
pub(crate) fn ptr_eq(a: &Value, b: &Value) -> Option<bool> {
    if !is_ptr(a) && !is_ptr(b) {
        return None;
    }
    Some(match (a, b) {
        (Value::Obj(x), Value::Obj(y)) => x == y,
        _ => false,
    })
}

/// Go's `==` on two interface operands: dynamic type first, value second.
///
/// The type half is [`go_type_name`], which is what `%T` prints — so a `pt` and
/// a `qt` with the same field are different types and unequal, and an interface
/// holding a nil *slice* is a `[]int` rather than a `<nil>` and so is not equal
/// to the untyped `nil` (Go's non-nil-interface-holding-nil rule falls out of
/// this rather than needing a case of its own).
///
/// The value half runs only once the types agree, so it never has to reconcile
/// two representations. A pointer compares by handle, the scalars by value, and
/// anything left — two structs of the same type, two strings — structurally,
/// which is the comparison the ordinary `==` path already performed. `Float` is
/// compared as `f64` rather than through the rendered string so that Go's
/// `NaN != NaN` survives.
///
/// Two interfaces holding a *slice*, *map* or *func* panic in Go ("comparing
/// uncomparable type []int"); go-rs answers structurally instead, which is
/// unchanged by this and not a case this function adds.
pub fn iface_eq(a: &Value, b: &Value) -> bool {
    if go_type_name(a) != go_type_name(b) {
        return false;
    }
    if let Some(same) = ptr_eq(a, b) {
        return same;
    }
    match (a, b) {
        (Value::Undef, Value::Undef) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x == y,
        _ => go_str(a) == go_str(b),
    }
}

/// [`GIFACE_EQ`] — `[a, b, ne]` → the interface comparison, negated when `ne`.
fn b_iface_eq(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let a = args.first().cloned().unwrap_or(Value::Undef);
    let b = args.get(1).cloned().unwrap_or(Value::Undef);
    let ne = args.get(2).map(|v| v.to_int() != 0).unwrap_or(false);
    Value::bool(iface_eq(&a, &b) != ne)
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
            Some(HostObj::Struct { fields, .. }) => {
                if let Some((_, v)) = fields.iter().find(|(f, _)| *f == name) {
                    return v.clone();
                }
                // Not a direct field — try an embedded one (Go field promotion).
                match embedded_field_owner(&h, id, &name) {
                    Some(owner) => match h.get(owner as usize) {
                        Some(HostObj::Struct { fields, .. }) => fields
                            .iter()
                            .find(|(f, _)| *f == name)
                            .map(|(_, v)| v.clone())
                            .unwrap_or(Value::Undef),
                        _ => Value::Undef,
                    },
                    None => Value::Undef,
                }
            }
            _ => {
                ffi_fault(vm, format!("go-rs: no field `{name}`"));
                Value::Undef
            }
        }
    })
}

/// Find the struct that declares `name` through `root`'s embedded fields, and
/// return its heap id. An embedded field is one whose field name equals the
/// type name of the struct it holds — exactly how the parser records
/// `struct { Base }`. The search is breadth-first so the shallowest depth wins,
/// matching Go's promotion rule.
fn embedded_field_owner(heap: &[HostObj], root: u32, name: &str) -> Option<u32> {
    let mut frontier = vec![root];
    // Embedding is acyclic in Go (a struct cannot embed itself by value), so
    // the walk terminates on the type graph's depth.
    for _ in 0..16 {
        let mut next = Vec::new();
        for id in frontier {
            let Some(HostObj::Struct { fields, .. }) = heap.get(id as usize) else {
                continue;
            };
            for (fname, fval) in fields {
                let Value::Obj(inner) = fval else { continue };
                let Some(HostObj::Struct {
                    type_name,
                    fields: inner_fields,
                    ..
                }) = heap.get(*inner as usize)
                else {
                    continue;
                };
                if fname != type_name {
                    continue; // a named field, not an embedded one
                }
                if inner_fields.iter().any(|(f, _)| f == name) {
                    return Some(*inner);
                }
                next.push(*inner);
            }
        }
        if next.is_empty() {
            return None;
        }
        frontier = next;
    }
    None
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
        // A write to a promoted field lands on the embedded struct that owns
        // it, not as a new field on the outer one.
        let target = match h.get(id as usize) {
            Some(HostObj::Struct { fields, .. }) if !fields.iter().any(|(f, _)| *f == name) => {
                embedded_field_owner(&h, id, &name).unwrap_or(id)
            }
            _ => id,
        };
        match h.get_mut(target as usize) {
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

/// Copy a struct value (Go value semantics). Non-struct values pass through
/// unchanged (slices/maps are reference types and must NOT be copied).
fn b_struct_copy(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    struct_copy(args.first().cloned().unwrap_or(Value::Undef))
}

/// Copy a fixed-size array value (Go array value semantics), given its written
/// element type. Non-array values pass through unchanged.
fn b_array_copy(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let elem_ty = args.get(1).map(go_str).unwrap_or_default();
    array_copy(args.first().cloned().unwrap_or(Value::Undef), &elem_ty)
}

/// One value copy for a value of written type `ty`: an array copy when `ty` is
/// `[N]T`, a struct copy otherwise (which is the identity on anything that is
/// not a struct, so a scalar or a reference passes straight through).
fn value_copy(v: Value, ty: &str) -> Value {
    match crate::ast::array_elem_ty(ty) {
        Some(elem) => array_copy(v, elem),
        None => struct_copy(v),
    }
}

/// One fixed-size array copy, recursing into elements that are themselves
/// values.
///
/// Go copies an array elementwise: `b := a` on a `[2][3]int` gives two
/// independent 2×3 arrays, and on a `[2]pt` two independent structs — while a
/// `[2][]int`'s two slices stay shared, because a slice is a reference. Which of
/// those an element is cannot be read off the heap (an array and a slice are the
/// same [`HostObj::Slice`]), so `elem_ty` — the *written* element type — decides
/// it. A struct element needs no such hint: [`struct_copy`] recognises one at
/// run time and is the identity on everything else.
/// The copy also inherits the source's `[N]T` tag, so `%T` still names the
/// array after it has been assigned, passed, returned or boxed into an `any` —
/// every one of which goes through a copy.
fn array_copy(v: Value, elem_ty: &str) -> Value {
    let Value::Obj(id) = v else { return v };
    let Some((_, _, len)) = slice_backing(id) else {
        return v;
    };
    let elems: Vec<Value> = (0..len)
        .map(|i| value_copy(slice_get(id, i).unwrap_or(Value::Undef), elem_ty))
        .collect();
    let arr_ty = HEAP.with(|h| match h.borrow().get(id as usize) {
        Some(HostObj::Slice { arr_ty, .. }) => arr_ty.clone(),
        _ => None,
    });
    Value::Obj(heap_alloc(HostObj::Slice {
        elems,
        arr_ty,
        elem_ty: None,
    }))
}

/// One struct value copy, recursing into the fields [`STRUCT_PLAN`] records as
/// struct-valued.
///
/// Go copies a struct *transitively*: assigning `outer{inner{1}, 2}` copies the
/// embedded `inner` too, so a write through the copy is invisible to the
/// original at every depth. Cloning only the field vector would share the
/// nested struct's handle and let a write through the copy reach the original
/// (`y := x; y.I.N = 8` used to change `x.I.N`).
///
/// The recursion is bounded without a visited set: Go rejects a struct type
/// that contains itself by value ("invalid recursive type"), so the plan graph
/// over value fields is a DAG. A pointer field breaks the cycle and is shared,
/// which is what makes a self-referential `*T` node type copy correctly.
fn struct_copy(v: Value) -> Value {
    let Value::Obj(id) = v else { return v };
    let parts = HEAP.with(|h| match h.borrow().get(id as usize) {
        Some(HostObj::Struct {
            type_name, fields, ..
        }) => Some((type_name.clone(), fields.clone())),
        // slice/map/other: a reference type, share the handle.
        _ => None,
    });
    let Some((type_name, mut fields)) = parts else {
        return v;
    };
    let nested = STRUCT_PLAN.with(|p| p.borrow().get(&type_name).cloned().unwrap_or_default());
    for (name, fv) in fields.iter_mut() {
        if let Some((_, fty)) = nested.iter().find(|(n, _)| n == name) {
            *fv = value_copy(fv.clone(), fty);
        }
    }
    // The copy is a struct *value*, never the pointer it was taken from, so it
    // compares field-wise again.
    Value::Obj(heap_alloc(HostObj::Struct {
        type_name,
        fields,
        by_ref: false,
    }))
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
    plain_panic(vm, format!("runtime error: {}", msg.into()));
}

/// A runtime fault whose message Go does *not* prefix with `runtime error: ` —
/// its `runtime.plainError` cases, such as writing to a nil map.
fn plain_panic(vm: &mut VM, full: String) {
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
    go_str_mode(v, FmtMode::V)
}

/// Which of `fmt`'s three struct-rendering verbs is being applied. They differ
/// only in how composites and strings are printed, so one walker serves all
/// three.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FmtMode {
    /// `%v` — `{1 a}`, bare strings.
    V,
    /// `%+v` — `{X:1 Y:a}`, field names added, strings still bare.
    PlusV,
    /// `%#v` — `main.P{X:1, Y:"a"}`, Go-syntax with types and quoted strings.
    SharpV,
}

pub(crate) fn go_str_mode(v: &Value, mode: FmtMode) -> String {
    match v {
        Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => format_float(*f),
        Value::Str(s) => {
            if mode == FmtMode::SharpV {
                go_quote(s.as_str())
            } else {
                s.as_str().to_string()
            }
        }
        // A nil operand renders as `<nil>` under all three verbs.
        Value::Undef => "<nil>".to_string(),
        Value::Obj(id) => obj_str_mode(*id, mode),
        other => other.as_str_cow().into_owned(),
    }
}

/// `%q` on a string: Go's `strconv.Quote` — double quotes, backslash escapes for
/// the C escapes and `\`/`"`, and `\xNN`/`\uNNNN` for other non-printables.
pub(crate) fn go_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{7}' => out.push_str("\\a"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\u{b}' => out.push_str("\\v"),
            c if !go_is_print(c) => out.push_str(&escape_rune(c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// How `strconv.Quote` writes a rune it will not print literally: `\xNN` for a
/// C0 control or `DEL`, `\uNNNN` inside the basic plane and `\UNNNNNNNN` above
/// it. The named escapes (`\n`, `\t`, …) are matched before this is reached.
fn escape_rune(c: u32) -> String {
    match c {
        0..=0x1f | 0x7f => format!("\\x{c:02x}"),
        0..=0xffff => format!("\\u{c:04x}"),
        _ => format!("\\U{c:08x}"),
    }
}

/// Whether Go's `strconv` writes a rune literally inside a quoted string —
/// `unicode.IsPrint`, which is every letter, mark, number, punctuation and
/// symbol plus the ASCII space.
///
/// The three non-printable classes tested here are exact: `Cc` (the C0 and C1
/// controls) is [`char::is_control`], every separator but the ASCII space is
/// [`char::is_whitespace`], and the private-use areas are three fixed ranges.
/// The fourth, `Cn` (code points Unicode has not assigned), needs the category
/// tables Rust's standard library does not expose, so an unassigned rune still
/// prints literally where Go escapes it — see BUGS.md.
fn go_is_print(c: char) -> bool {
    if c.is_control() {
        return false;
    }
    if c == ' ' {
        return true;
    }
    if c.is_whitespace() {
        return false;
    }
    !matches!(c as u32, 0xe000..=0xf8ff | 0xf0000..=0xffffd | 0x100000..=0x10fffd)
}

/// `%q` on an integer: Go quotes it as a rune literal (`'A'`, `'\n'`, `'世'`).
pub(crate) fn go_quote_rune(n: i64) -> String {
    let Some(c) = u32::try_from(n).ok().and_then(char::from_u32) else {
        // Go renders an out-of-range code point as the replacement character.
        return "'\u{fffd}'".to_string();
    };
    let inner = go_quote(&c.to_string());
    // Reuse the string escaper, then swap the delimiters and fix `'`/`"`.
    let body = &inner[1..inner.len() - 1];
    let body = body.replace("\\\"", "\"").replace('\'', "\\'");
    format!("'{body}'")
}

/// `%T`: the value's Go type name. go-rs carries no static element type for a
/// slice or map, so those are described from the values actually present and an
/// empty one falls back to `interface {}`.
///
/// A fixed-size array is the exception: its written `[N]T` is stamped on the
/// object, because the length is not recoverable from the elements and a
/// `[3]int` would otherwise be indistinguishable from a 3-element `[]int`.
pub(crate) fn go_type_name(v: &Value) -> String {
    match v {
        Value::Bool(_) => "bool".to_string(),
        Value::Int(_) => "int".to_string(),
        Value::Float(_) => "float64".to_string(),
        Value::Str(_) => "string".to_string(),
        Value::Undef => "<nil>".to_string(),
        Value::Obj(id) => HEAP.with(|h| {
            let h = h.borrow();
            match h.get(*id as usize) {
                // A `fmt`-tagged slice names its written element type, which is
                // the only way `[]uint8` and `[]int32` are distinguishable from
                // `[]int` — their elements are all plain integers.
                Some(HostObj::Slice {
                    elems: a,
                    arr_ty: None,
                    elem_ty,
                }) => match elem_ty {
                    Some(t) => format!("[]{}", go_type_spelling(t)),
                    None => format!("[]{}", elem_type_name(a.first())),
                },
                Some(HostObj::Slice {
                    arr_ty: Some(ty), ..
                }) => ty.clone(),
                Some(HostObj::SliceView {
                    backing, offset, ..
                }) => {
                    let e = match h.get(*backing as usize) {
                        Some(HostObj::Slice { elems: a, .. }) => a.get(*offset).cloned(),
                        _ => None,
                    };
                    format!("[]{}", elem_type_name(e.as_ref()))
                }
                Some(HostObj::Map(m)) => format!(
                    "map[{}]{}",
                    elem_type_name(m.first().map(|(k, _)| k)),
                    elem_type_name(m.first().map(|(_, v)| v))
                ),
                // A user-declared struct is qualified by its package, and go-rs
                // only ever compiles `package main`.
                Some(HostObj::Struct { type_name, .. }) => format!("main.{type_name}"),
                Some(HostObj::Closure { .. }) => "func()".to_string(),
                Some(HostObj::Cell(v)) => go_type_name(v),
                // A defined type is named, not described: `main.Weekday`, never
                // the `int` it is represented as.
                Some(HostObj::Named { ty, .. }) => go_type_spelling(ty),
                // A typed nil records the type it was written as, so unlike a
                // populated slice or map it needs no guess from its contents.
                Some(HostObj::Nil { ty, .. }) => go_type_spelling(ty),
                Some(HostObj::F32(_)) => "float32".to_string(),
                Some(HostObj::U64 { ty, .. }) => ty.clone(),
                // Every receive site maps the sentinel away, so it is only
                // reachable if one was missed; name it after what it stands for.
                Some(HostObj::ChanClosed) => "<nil>".to_string(),
                None => "<nil>".to_string(),
            }
        }),
        _ => "interface {}".to_string(),
    }
}

fn elem_type_name(v: Option<&Value>) -> String {
    match v {
        Some(v) => go_type_name(v),
        None => "interface {}".to_string(),
    }
}

/// How `%T` spells a written type.
///
/// `byte` and `rune` are Go aliases and `%T` prints the type they name, so a
/// `[]byte` is `[]uint8` and a `[]rune` is `[]int32`. Any identifier that is not
/// predeclared names a type declared in the program, which `%T` qualifies by its
/// package — always `main` here. Both are applied per identifier, so
/// `map[string]pt` renames only the element.
pub(crate) fn go_type_spelling(ty: &str) -> String {
    let mut out = String::with_capacity(ty.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        match word.as_str() {
            "" => {}
            "byte" => out.push_str("uint8"),
            "rune" => out.push_str("int32"),
            w if is_predeclared_type(w) => out.push_str(w),
            w => {
                out.push_str("main.");
                out.push_str(w);
            }
        }
        word.clear();
    };
    for c in ty.chars() {
        if c.is_alphanumeric() || c == '_' {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// Whether `w` is one of Go's predeclared type names (plus the keywords that
/// open a type literal), which `%T` prints unqualified. Everything else is
/// declared in the program and carries its package.
fn is_predeclared_type(w: &str) -> bool {
    matches!(
        w,
        "bool"
            | "string"
            | "int"
            | "int8"
            | "int16"
            | "int32"
            | "int64"
            | "uint"
            | "uint8"
            | "uint16"
            | "uint32"
            | "uint64"
            | "uintptr"
            | "float32"
            | "float64"
            | "complex64"
            | "complex128"
            | "error"
            | "any"
            | "interface"
            | "struct"
            | "map"
            | "chan"
            | "func"
    ) || w.chars().all(|c| c.is_ascii_digit())
}

/// The element type of a written container type — `[]T` and `[N]T` name `T`,
/// `map[K]V` names `V` — or `None` when the type is not a container. The key of
/// a `map[K]V` can itself carry brackets (`map[[2]int]V`), so the key ends at
/// the `]` closing the one `map[` opened, found by depth.
fn elem_ty_spelling(ty: &str) -> Option<String> {
    if let Some(rest) = ty.strip_prefix("[]") {
        return Some(rest.to_string());
    }
    if let Some(rest) = ty.strip_prefix("map[") {
        let mut depth = 0usize;
        for (i, c) in rest.char_indices() {
            match c {
                '[' => depth += 1,
                ']' if depth == 0 => return Some(rest[i + 1..].to_string()),
                ']' => depth -= 1,
                _ => {}
            }
        }
        return None;
    }
    // `[N]T` — a fixed-size array, whose length is digits between the brackets.
    let rest = ty.strip_prefix('[')?;
    let close = rest.find(']')?;
    rest[..close]
        .chars()
        .all(|c| c.is_ascii_digit())
        .then(|| rest[close + 1..].to_string())
}

/// The map key type of a written `map[K]V`, or `None` for any other type.
fn key_ty_spelling(ty: &str) -> Option<String> {
    let rest = ty.strip_prefix("map[")?;
    let mut depth = 0usize;
    for (i, c) in rest.char_indices() {
        match c {
            '[' => depth += 1,
            ']' if depth == 0 => return Some(rest[..i].to_string()),
            ']' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// [`GNAMED_BOX`] — tag a `fmt` argument with the defined type it was written
/// as. Stack `[value, type]`.
fn b_named_box(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let inner = args.first().cloned().unwrap_or(Value::Undef);
    let ty = args.get(1).map(go_str).unwrap_or_default();
    Value::Obj(heap_alloc(HostObj::Named { ty, inner }))
}

/// The value inside a [`HostObj::Named`] tag, or the value itself. Every part of
/// the formatter but `%T` and `%#v` reads through the tag: a defined type is its
/// base at run time, so it prints, distributes and coerces as the base does.
pub(crate) fn unname(v: &Value) -> Value {
    let Value::Obj(id) = v else { return v.clone() };
    HEAP.with(|h| match h.borrow().get(*id as usize) {
        Some(HostObj::Named { inner, .. }) => inner.clone(),
        _ => v.clone(),
    })
}

/// [`GELEM_TAG`] — stamp the written element type on every slice inside a `fmt`
/// argument. Stack `[value, type]`, where `type` is the operand's written type.
fn b_elem_tag(vm: &mut VM, argc: u8) -> Value {
    let args = pop_args(vm, argc);
    let v = args.first().cloned().unwrap_or(Value::Undef);
    let ty = args.get(1).map(go_str).unwrap_or_default();
    tag_elem_ty(&v, &ty)
}

/// Rebuild `v` with each slice node carrying the element type `ty` names at that
/// depth. Like the width boxes, the result is a display copy: the program's own
/// slice keeps no tag, so nothing it can observe changes.
fn tag_elem_ty(v: &Value, ty: &str) -> Value {
    let Some(elem) = elem_ty_spelling(ty) else {
        return v.clone();
    };
    // A nil slice or map is not a rebuildable composite: it prints as `[]` /
    // `map[]` but `%#v` names it `[]int(nil)`, which only the nil object knows.
    if nil_composite_kind(v).is_some() {
        return v.clone();
    }
    if let Some(pairs) = map_pairs(v) {
        let key = key_ty_spelling(ty).unwrap_or_default();
        let tagged = pairs
            .into_iter()
            .map(|(k, val)| (tag_elem_ty(&k, &key), tag_elem_ty(&val, &elem)))
            .collect();
        return Value::Obj(heap_alloc(HostObj::Map(tagged)));
    }
    let Some(es) = slice_elems(v) else {
        return v.clone();
    };
    let elems: Vec<Value> = es.iter().map(|e| tag_elem_ty(e, &elem)).collect();
    // The rebuild keeps the `[N]T` tag, or `%T` on a tagged array would fall
    // back to naming it a slice.
    let arr_ty = match v {
        Value::Obj(id) => HEAP.with(|h| match h.borrow().get(*id as usize) {
            Some(HostObj::Slice { arr_ty, .. }) => arr_ty.clone(),
            _ => None,
        }),
        _ => None,
    };
    Value::Obj(heap_alloc(HostObj::Slice {
        elems,
        arr_ty,
        elem_ty: Some(elem),
    }))
}

/// The elements of a slice value (materialising a sub-slice view), or `None`
/// when the value is not a slice. Lets a verb distribute over a slice the way
/// `fmt` does for `%d`/`%x`.
pub(crate) fn slice_elems(v: &Value) -> Option<Vec<Value>> {
    let Value::Obj(id) = v else { return None };
    HEAP.with(|h| {
        let h = h.borrow();
        match h.get(*id as usize) {
            Some(HostObj::Slice { elems: a, .. }) => Some(a.clone()),
            Some(HostObj::SliceView {
                backing,
                offset,
                len,
                ..
            }) => match h.get(*backing as usize) {
                Some(HostObj::Slice { elems: a, .. }) => Some(a[*offset..*offset + *len].to_vec()),
                _ => Some(Vec::new()),
            },
            Some(HostObj::Nil {
                kind: NilKind::Slice,
                ..
            }) => Some(Vec::new()),
            _ => None,
        }
    })
}

/// A value's bytes for the text-oriented verbs (`%s`, `%q`): a string's UTF-8,
/// or a slice whose elements are all byte-valued integers — go-rs has no static
/// element type, so a `[]byte` is recognised by its contents. A slice holding
/// anything outside `0..=255` is not text and yields `None`.
pub(crate) fn bytes_of(v: &Value) -> Option<Vec<u8>> {
    match v {
        Value::Str(s) => Some(s.as_str().as_bytes().to_vec()),
        _ => {
            let es = slice_elems(v)?;
            es.iter()
                .map(|e| match e {
                    Value::Int(n) if (0..=255).contains(n) => Some(*n as u8),
                    _ => None,
                })
                .collect()
        }
    }
}

/// Format a heap object the way Go's `%v` does: `[e0 e1 …]` for a slice,
/// `map[k0:v0 …]` (keys sorted, as Go's fmt does) for a map, `{f0 f1 …}` for a
/// struct.
fn obj_str_mode(id: u32, mode: FmtMode) -> String {
    // `%#v` uses Go source syntax: a typed composite literal, `, `-separated,
    // with every element itself in Go syntax. `%v`/`%+v` use the space-separated
    // display forms, differing only in whether struct fields are named.
    let sharp = mode == FmtMode::SharpV;
    let sep = if sharp { ", " } else { " " };
    let elems = |vs: &[Value]| -> String {
        vs.iter()
            .map(|v| go_str_mode(v, mode))
            .collect::<Vec<_>>()
            .join(sep)
    };
    HEAP.with(|h| {
        let h = h.borrow();
        match h.get(id as usize) {
            Some(HostObj::Slice {
                elems: a, elem_ty, ..
            }) => {
                if !sharp {
                    return format!("[{}]", elems(a));
                }
                // `%#v` of a byte slice writes Go source for one: the alias name
                // `[]byte` (where `%T` prints `[]uint8`) and hex element
                // literals.
                if elem_ty.as_deref().is_some_and(is_byte_elem) {
                    let bytes: Vec<String> =
                        a.iter().map(|e| format!("{:#04x}", e.to_int())).collect();
                    return format!("[]byte{{{}}}", bytes.join(", "));
                }
                format!("{}{{{}}}", go_type_name(&Value::Obj(id)), elems(a))
            }
            Some(HostObj::SliceView {
                backing,
                offset,
                len,
                ..
            }) => {
                let view: Vec<Value> = match h.get(*backing as usize) {
                    Some(HostObj::Slice { elems: a, .. }) => a[*offset..*offset + *len].to_vec(),
                    _ => Vec::new(),
                };
                if sharp {
                    format!("{}{{{}}}", go_type_name(&Value::Obj(id)), elems(&view))
                } else {
                    format!("[{}]", elems(&view))
                }
            }
            Some(HostObj::Map(m)) => {
                // `fmt` sorts map keys so map output is deterministic. Sorting
                // the rendered `k:v` strings is only right when the keys order
                // the same as their text, so sort on the key values themselves.
                let mut pairs: Vec<(String, String)> = m
                    .iter()
                    .map(|(k, v)| (go_str_mode(k, mode), go_str_mode(v, mode)))
                    .collect();
                pairs.sort_by(|a, b| map_key_cmp(&a.0, &b.0));
                let body = pairs
                    .into_iter()
                    .map(|(k, v)| format!("{k}:{v}"))
                    .collect::<Vec<_>>()
                    .join(sep);
                if sharp {
                    format!("{}{{{}}}", go_type_name(&Value::Obj(id)), body)
                } else {
                    format!("map[{body}]")
                }
            }
            // A `float32` prints its own width's shortest decimal.
            Some(HostObj::F32(f)) => format_float32(*f),
            // An unsigned 64-bit integer prints its unsigned digits.
            Some(HostObj::U64 { val, .. }) => val.to_string(),
            // Only reachable if a receive site failed to map the sentinel away;
            // it stands for "no value", which is what Go's nil prints as.
            Some(HostObj::ChanClosed) => "<nil>".to_string(),
            // Go prints a nil slice as `[]` and a nil map as `map[]` — the same
            // as an empty one — and `%#v` as the type followed by `(nil)`.
            Some(HostObj::Nil { kind, ty }) => match (mode, kind) {
                (FmtMode::SharpV, _) => format!("{ty}(nil)"),
                (_, NilKind::Slice) => "[]".to_string(),
                (_, NilKind::Map) => "map[]".to_string(),
            },
            Some(HostObj::Struct {
                type_name, fields, ..
            }) => match mode {
                FmtMode::V => {
                    let parts: Vec<String> =
                        fields.iter().map(|(_, v)| go_str_mode(v, mode)).collect();
                    format!("{{{}}}", parts.join(" "))
                }
                FmtMode::PlusV => {
                    let parts: Vec<String> = fields
                        .iter()
                        .map(|(n, v)| format!("{n}:{}", go_str_mode(v, mode)))
                        .collect();
                    format!("{{{}}}", parts.join(" "))
                }
                FmtMode::SharpV => {
                    let parts: Vec<String> = fields
                        .iter()
                        .map(|(n, v)| format!("{n}:{}", go_str_mode(v, mode)))
                        .collect();
                    format!("main.{type_name}{{{}}}", parts.join(", "))
                }
            },
            // Go prints a function value as a hex pointer; a fixed marker suffices.
            Some(HostObj::Closure { .. }) => "<func>".to_string(),
            // A cell is an internal box; render its contents (a captured value).
            Some(HostObj::Cell(v)) => go_str_mode(v, mode),
            // A defined type prints as its base — except under `%#v`, where a
            // composite is written as a typed literal and the type is the
            // defined name (`main.mySlice{1, 2}`). A scalar's `%#v` carries no
            // type at all, so it needs nothing.
            Some(HostObj::Named { ty, inner }) => {
                let body = go_str_mode(inner, mode);
                let composite = slice_elems(inner).is_some() || map_pairs(inner).is_some();
                match body.find('{').filter(|_| sharp && composite) {
                    Some(i) => format!("{}{}", go_type_spelling(ty), &body[i..]),
                    None => body,
                }
            }
            None => "<nil>".to_string(),
        }
    })
}

/// Order two rendered map keys the way `fmt` orders the underlying values:
/// numerically when both parse as numbers, lexicographically otherwise. Plain
/// string sorting would put `10` before `9`.
fn map_key_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(x), Ok(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
        _ => a.cmp(b),
    }
}

/// Go prints floats via `strconv.FormatFloat(f, 'g', -1, 64)`: shortest exact
/// decimal, whole values without a fractional part (`3`, not `3.0`), and
/// `+Inf`/`-Inf`/`NaN` for the non-finite cases.
pub(crate) fn format_float(f: f64) -> String {
    if f.is_nan() {
        return "NaN".to_string();
    }
    if f.is_infinite() {
        return if f < 0.0 { "-Inf" } else { "+Inf" }.to_string();
    }
    // Go's `ftoa` shortest-`g` path picks the `%e` form when the decimal
    // exponent is `< -4` or `>= 6`, and the `%f` form otherwise (strconv/ftoa.go
    // sets `eprec = 6` for the shortest case). So `999999` prints as `999999`
    // but `1000000` prints as `1e+06`, and `0.0001` stays decimal while
    // `0.00001` becomes `1e-05`.
    let (mant, exp) = shortest_sci(f);
    if !(-4..6).contains(&exp) {
        format_e(&mant, exp, 'e')
    } else {
        // Inside the plain-decimal window Rust's `{}` is the same shortest
        // round-tripping decimal Go computes, and never switches to exponent
        // notation itself.
        format!("{f}")
    }
}

/// Go's `strconv.FormatFloat(f, 'g', -1, 32)` — the same rendering as
/// [`format_float`], but the shortest decimal is computed against **32-bit**
/// precision. That is the whole difference: the `f64` nearest `1/3` prints as
/// `0.3333333333333333` at 64 bits and `0.33333334` at 32.
pub(crate) fn format_float32(f: f32) -> String {
    if f.is_nan() {
        return "NaN".to_string();
    }
    if f.is_infinite() {
        return if f < 0.0 { "-Inf" } else { "+Inf" }.to_string();
    }
    let (mant, exp) = shortest_sci_32(f);
    if !(-4..6).contains(&exp) {
        format_e(&mant, exp, 'e')
    } else {
        plain_form(&mant, exp)
    }
}

/// A finite `f32`'s shortest round-tripping decimal mantissa and exponent.
///
/// Rust's `{:e}` picks the same *length* Go does — both emit the shortest
/// decimal that round-trips — but breaks a tie the other way, so the result is
/// passed through [`go_even_tie`].
fn shortest_sci_32(f: f32) -> (String, i32) {
    let s = format!("{f:e}");
    let (mant, exp) = s.split_once('e').unwrap_or((s.as_str(), "0"));
    (go_even_tie(mant, f), exp.parse().unwrap_or(0))
}

/// Go breaks a shortest-decimal tie towards the **even** last digit; Rust's
/// formatter breaks it away from zero. They can only differ when Rust's last
/// digit is odd, so only then is the exact expansion consulted — and a tie is
/// exactly "the digits past the shortest form are a single 5".
///
/// `float32(4025693.25)` is such a value: Go prints `4.0256932e+06`, Rust
/// `4.0256933e+06`.
fn go_even_tie(mant: &str, f: f32) -> String {
    let Some(last) = mant.chars().last() else {
        return mant.to_string();
    };
    if !matches!(last, '1' | '3' | '5' | '7' | '9') {
        return mant.to_string();
    }
    let digits: String = mant.chars().filter(char::is_ascii_digit).collect();
    let exact = exact_sig_digits(f);
    // A tie: one more exact digit than the shortest form, and it is a 5 over the
    // shortest form's *lower* neighbour (Rust rounded up to reach `digits`).
    let lower = format!("{}{}", &digits[..digits.len() - 1], step_down(last));
    if exact.len() != digits.len() + 1 || !exact.ends_with('5') || exact[..digits.len()] != lower {
        return mant.to_string();
    }
    let mut out: Vec<char> = mant.chars().collect();
    if let Some(c) = out.last_mut() {
        *c = step_down(last);
    }
    out.into_iter().collect()
}

/// The digit one below `d`. Only called on an odd digit, so it never borrows.
fn step_down(d: char) -> char {
    char::from(d as u8 - 1)
}

/// Every significant digit of a finite `f32`'s **exact** value, leading and
/// trailing zeros trimmed. A binary float's decimal expansion terminates, and
/// the longest an `f32`'s can be is the 149 fractional digits of the smallest
/// subnormal — so 160 places renders every one of them exactly.
fn exact_sig_digits(f: f32) -> String {
    let s = format!("{:.*}", 160, f64::from(f).abs());
    let digits: String = s.chars().filter(char::is_ascii_digit).collect();
    digits
        .trim_start_matches('0')
        .trim_end_matches('0')
        .to_string()
}

/// Assemble the plain (non-exponent) rendering of a mantissa and exponent —
/// `("1.5", 3)` is `1500`, `("1.5", -2)` is `0.015`.
fn plain_form(mant: &str, exp: i32) -> String {
    let (sign, m) = match mant.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", mant),
    };
    let digits: String = m.chars().filter(char::is_ascii_digit).collect();
    let point = exp + 1; // digits before the decimal point
    if point <= 0 {
        format!(
            "{sign}0.{}{digits}",
            "0".repeat(point.unsigned_abs() as usize)
        )
    } else if point as usize >= digits.len() {
        format!(
            "{sign}{digits}{}",
            "0".repeat(point as usize - digits.len())
        )
    } else {
        format!(
            "{sign}{}.{}",
            &digits[..point as usize],
            &digits[point as usize..]
        )
    }
}

/// Split a finite `f64` into its shortest round-tripping decimal mantissa
/// (`"-1.2345"`, sign included) and decimal exponent, via Rust's `{:e}` — which
/// already emits the shortest such representation.
fn shortest_sci(f: f64) -> (String, i32) {
    let s = format!("{f:e}");
    match s.split_once('e') {
        Some((m, e)) => (m.to_string(), e.parse().unwrap_or(0)),
        None => (s, 0),
    }
}

/// Assemble Go's `%e` rendering from a mantissa and exponent: the exponent
/// always carries a sign and at least two digits (`1e+06`, `1.5e-07`,
/// `1e+100`).
fn format_e(mant: &str, exp: i32, e: char) -> String {
    let sign = if exp < 0 { '-' } else { '+' };
    let mag = exp.unsigned_abs();
    if mag < 10 {
        format!("{mant}{e}{sign}0{mag}")
    } else {
        format!("{mant}{e}{sign}{mag}")
    }
}

/// Go's `%e`/`%E` verb: `prec` digits after the mantissa's decimal point
/// (default 6), or the shortest round-tripping mantissa when `prec` is `None`
/// and the caller asked for `%v`-style shortest output.
fn format_float_e(f: f64, prec: Option<usize>, upper: bool) -> String {
    if f.is_nan() {
        return "NaN".to_string();
    }
    if f.is_infinite() {
        return if f < 0.0 { "-Inf" } else { "+Inf" }.to_string();
    }
    let e = if upper { 'E' } else { 'e' };
    match prec {
        None => {
            let (mant, exp) = shortest_sci(f);
            format_e(&mant, exp, e)
        }
        Some(p) => {
            // Rust's `{:.*e}` rounds the mantissa to `p` fractional digits with
            // the same semantics, so only the exponent spelling differs.
            let s = format!("{:.*e}", p, f);
            match s.split_once('e') {
                Some((m, ex)) => format_e(m, ex.parse().unwrap_or(0), e),
                None => s,
            }
        }
    }
}

/// Go's `%g`/`%G` verb. With no explicit precision this is the same shortest
/// representation `%v` uses; with a precision it means "`prec` significant
/// digits", and the `%e`-vs-`%f` choice compares the decimal exponent against
/// that precision instead of the shortest-case 6.
fn format_float_g(f: f64, prec: Option<usize>, upper: bool) -> String {
    if f.is_nan() {
        return "NaN".to_string();
    }
    if f.is_infinite() {
        return if f < 0.0 { "-Inf" } else { "+Inf" }.to_string();
    }
    let Some(p) = prec else {
        let s = format_float(f);
        return if upper { s.replace('e', "E") } else { s };
    };
    // Go treats `%.0g` as `%.1g` (at least one significant digit).
    let p = p.max(1);
    // Round to `p` significant digits first, then decide the form from the
    // rounded exponent — rounding can carry into a new decade (9.99 → 1e+01).
    let (mant, exp) = match format!("{:.*e}", p - 1, f).split_once('e') {
        Some((m, e)) => (m.to_string(), e.parse::<i32>().unwrap_or(0)),
        None => (format!("{f}"), 0),
    };
    let e = if upper { 'E' } else { 'e' };
    if exp < -4 || exp >= p as i32 {
        // `%g` strips a trailing zero run from the mantissa that `%e` keeps.
        format_e(trim_zeros(&mant), exp, e)
    } else {
        let decimals = (p as i32 - 1 - exp).max(0) as usize;
        trim_zeros(&format!("{:.*}", decimals, f)).to_string()
    }
}

/// Drop a trailing fractional zero run (and a bare trailing `.`) from a decimal
/// string, the way Go's `%g` does. Leaves integral strings untouched.
fn trim_zeros(s: &str) -> &str {
    if !s.contains('.') {
        return s;
    }
    s.trim_end_matches('0').trim_end_matches('.')
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

/// The flags, width and precision of one `%` verb, as written before it.
#[derive(Clone, Copy, Default)]
struct Spec {
    /// `-`: pad on the right instead of the left.
    left: bool,
    /// `0`: pad a numeric verb with leading zeros, after any sign.
    zero: bool,
    /// `+`: always print a sign.
    plus: bool,
    /// `#`: the alternate form (`0x` on hex, back-quotes on `%q`, Go syntax on `%v`).
    sharp: bool,
    width: Option<usize>,
    prec: Option<usize>,
}

/// `fmt.Printf`: the first argument is the format string and each verb consumes
/// the next operand.
///
/// `%v` and `%T` describe an operand as a whole. Every other verb applies
/// *element-wise* to a composite operand — [`render_verb`] does that — so the
/// flags, width and precision parsed here belong to each element rather than to
/// the whole rendering: `%8q` of a `[]string{"a", "b"}` is `[     "a"      "b"]`.
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
        let mut spec = Spec::default();
        while i < chars.len() {
            match chars[i] {
                '-' => spec.left = true,
                '0' => spec.zero = true,
                '+' => spec.plus = true,
                '#' => spec.sharp = true,
                ' ' => {}
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
        spec.width = has_width.then_some(width);
        // precision
        if i < chars.len() && chars[i] == '.' {
            i += 1;
            let mut p = 0usize;
            while i < chars.len() && chars[i].is_ascii_digit() {
                p = p * 10 + (chars[i] as usize - '0' as usize);
                i += 1;
            }
            spec.prec = Some(p);
        }
        if i >= chars.len() {
            break;
        }
        let verb = chars[i];
        i += 1;

        match verb {
            '%' => out.push('%'),
            // `%T` is the operand's type, not its value.
            'T' => out.push_str(&pad(
                &rest.next().map(go_type_name).unwrap_or_default(),
                'T',
                &spec,
            )),
            // `%v` and anything not a known verb render the whole value, with
            // `+`/`#` selecting the `%+v` / `%#v` forms and precision truncating
            // a string.
            _ if !is_verb(verb) => {
                let mode = if spec.sharp {
                    FmtMode::SharpV
                } else if spec.plus {
                    FmtMode::PlusV
                } else {
                    FmtMode::V
                };
                let mut s = rest
                    .next()
                    .map(|v| go_str_mode(v, mode))
                    .unwrap_or_default();
                if let Some(p) = spec.prec {
                    if s.chars().count() > p {
                        s = s.chars().take(p).collect();
                    }
                }
                out.push_str(&pad(&s, verb, &spec));
            }
            _ => match rest.next() {
                Some(v) => out.push_str(&render_verb(v, verb, &spec, 0)),
                None => out.push_str(&pad(missing_operand(verb), verb, &spec)),
            },
        }
    }
    out
}

/// Whether `c` is a verb [`render_verb`] renders element-wise. `v` and `T` are
/// deliberately absent: they describe a composite as a whole.
fn is_verb(c: char) -> bool {
    matches!(
        c,
        't' | 'q'
            | 'f'
            | 'F'
            | 'e'
            | 'E'
            | 'g'
            | 'G'
            | 'd'
            | 'x'
            | 'X'
            | 'o'
            | 'b'
            | 'c'
            | 'U'
            | 's'
    )
}

/// What a verb renders with no operand left. (Go writes `%!d(MISSING)`; go-rs
/// keeps the verb's zero value, which is what the rest of the formatter assumes
/// for an absent argument.)
fn missing_operand(verb: char) -> &'static str {
    match verb {
        'q' => "\"\"",
        's' | 'c' | 't' => "",
        'f' | 'F' => "0.000000",
        'e' => "0.000000e+00",
        'E' => "0.000000E+00",
        'U' => "U+0000",
        _ => "0",
    }
}

/// Render one operand under `verb`, distributing over a composite.
///
/// Go applies a verb to each element of a slice, array, map or struct and wraps
/// the results the way `%v` would — `[e0 e1]`, `map[k0:v0]`, `{f0 f1}` — with
/// the verb's flags, width and precision applied to each element. The exception
/// is a byte slice under `%s`/`%q`/`%x`/`%X`, which is the text it holds and not
/// a list; that holds at every depth, so a `[][]byte` prints its rows as
/// strings.
///
/// `depth` is 0 only for the operand itself: a nil *inside* a composite prints
/// `<nil>` where a nil operand is the bad-verb form `%!q(<nil>)`.
fn render_verb(v: &Value, verb: char, spec: &Spec, depth: usize) -> String {
    // A defined type is its base to every verb here — only `%T` and `%#v`, which
    // do not come through this path, print the name.
    let unnamed = unname(v);
    let v = &unnamed;
    if matches!(verb, 's' | 'q' | 'x' | 'X') {
        if let Some(b) = slice_bytes(v) {
            let text = Value::str(String::from_utf8_lossy(&b).into_owned());
            return pad(&scalar_verb(&text, verb, spec), verb, spec);
        }
    }
    if let Some(es) = slice_elems(v) {
        let body: Vec<String> = es
            .iter()
            .map(|e| render_verb(e, verb, spec, depth + 1))
            .collect();
        return format!("[{}]", body.join(" "));
    }
    if let Some(pairs) = map_pairs(v) {
        // `fmt` orders map output by the *key values*, then renders each key and
        // value with the verb — so the sort reads the plain `%v` spelling, which
        // a bad-verb wrapper around the rendered key would otherwise scramble.
        let mut rows: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, val)| {
                (
                    go_str(k),
                    format!(
                        "{}:{}",
                        render_verb(k, verb, spec, depth + 1),
                        render_verb(val, verb, spec, depth + 1)
                    ),
                )
            })
            .collect();
        rows.sort_by(|a, b| map_key_cmp(&a.0, &b.0));
        let body: Vec<String> = rows.into_iter().map(|(_, r)| r).collect();
        return format!("map[{}]", body.join(" "));
    }
    if let Some(fields) = struct_fields_of(v) {
        let body: Vec<String> = fields
            .iter()
            .map(|(_, f)| render_verb(f, verb, spec, depth + 1))
            .collect();
        return format!("{{{}}}", body.join(" "));
    }
    // An empty composite still has the shape of one: a nil slice under a
    // non-string verb is `[]`, a nil map `map[]`.
    if let Some(kind) = nil_composite_kind(v) {
        return match kind {
            NilKind::Slice => "[]".to_string(),
            NilKind::Map => "map[]".to_string(),
        };
    }
    if let Some(bad) = bad_verb(v, verb, spec, depth) {
        return bad;
    }
    pad(&scalar_verb(v, verb, spec), verb, spec)
}

/// The bytes of a value `fmt` reads as text under `%s`/`%q`/`%x`: a `[]byte`.
///
/// The written element type decides it when the compiler knew one ([`GELEM_TAG`]
/// stamps it); otherwise the elements are guessed from, which is right for the
/// `[]byte` an untyped path produced and wrong for a `[]int` that happens to
/// hold small numbers. An empty untagged slice is *not* guessed to be bytes:
/// the guess is vacuously true there, and `[]` is the commoner answer.
fn slice_bytes(v: &Value) -> Option<Vec<u8>> {
    if let Value::Obj(id) = v {
        // A nil `[]byte` is the empty string, a nil `[]int` the empty list.
        let nil_ty = HEAP.with(|h| match h.borrow().get(*id as usize) {
            Some(HostObj::Nil { kind, ty }) => Some((*kind, ty.clone())),
            _ => None,
        });
        if let Some((kind, ty)) = nil_ty {
            let bytes =
                kind == NilKind::Slice && elem_ty_spelling(&ty).is_some_and(|e| is_byte_elem(&e));
            return bytes.then(Vec::new);
        }
    }
    let tagged = slice_elem_tag(v);
    if matches!(&tagged, Some(t) if !is_byte_elem(t)) {
        return None;
    }
    let es = slice_elems(v)?;
    if tagged.is_none() && es.is_empty() {
        return None;
    }
    es.iter()
        .map(|e| match e {
            Value::Int(n) if (0..=255).contains(n) => Some(*n as u8),
            _ => None,
        })
        .collect()
}

/// Whether a written element type spells one of Go's byte types.
fn is_byte_elem(t: &str) -> bool {
    matches!(t, "byte" | "uint8")
}

/// The element type [`GELEM_TAG`] stamped on a slice, if any.
fn slice_elem_tag(v: &Value) -> Option<String> {
    let Value::Obj(id) = v else { return None };
    HEAP.with(|h| match h.borrow().get(*id as usize) {
        Some(HostObj::Slice { elem_ty, .. }) => elem_ty.clone(),
        _ => None,
    })
}

/// Which kind of nil composite a value is, or `None` when it is not one.
fn nil_composite_kind(v: &Value) -> Option<NilKind> {
    let Value::Obj(id) = v else { return None };
    HEAP.with(|h| match h.borrow().get(*id as usize) {
        Some(HostObj::Nil { kind, .. }) => Some(*kind),
        _ => None,
    })
}

/// A struct's `(field, value)` pairs, or `None` when the value is not a struct.
fn struct_fields_of(v: &Value) -> Option<Vec<(String, Value)>> {
    let Value::Obj(id) = v else { return None };
    HEAP.with(|h| match h.borrow().get(*id as usize) {
        Some(HostObj::Struct { fields, .. }) => Some(fields.clone()),
        _ => None,
    })
}

/// Render one non-composite operand under `verb`, unpadded.
fn scalar_verb(v: &Value, verb: char, spec: &Spec) -> String {
    match verb {
        't' => go_str(v),
        // `%q` quotes a string with `strconv.Quote` and an integer as a rune
        // literal. `#` asks for a back-quoted string where one is possible.
        'q' => match v {
            Value::Int(n) => go_quote_rune(*n),
            _ => {
                let mut s = go_str(v);
                // Precision truncates the string before it is quoted, so `%.2q`
                // of `"alpha"` is `"al"` — the quotes are not part of the count.
                if let Some(p) = spec.prec {
                    if s.chars().count() > p {
                        s = s.chars().take(p).collect();
                    }
                }
                if spec.sharp && !s.contains('`') && !s.contains('\n') {
                    format!("`{s}`")
                } else {
                    go_quote(&s)
                }
            }
        },
        'f' | 'F' => {
            let x = arg_float(v);
            let s = if x.is_nan() {
                "NaN".to_string()
            } else if x.is_infinite() {
                if x < 0.0 { "-Inf" } else { "+Inf" }.to_string()
            } else {
                format!("{:.*}", spec.prec.unwrap_or(6), x)
            };
            signed(s, spec)
        }
        'e' | 'E' => signed(
            format_float_e(arg_float(v), Some(spec.prec.unwrap_or(6)), verb == 'E'),
            spec,
        ),
        // `%g` with no precision is the shortest round-tripping decimal —
        // computed at the operand's own width, so a `float32` box narrows it.
        'g' | 'G' => {
            let s = match (spec.prec, unbox_f32(v)) {
                (None, Some(f)) => {
                    let g = format_float32(f);
                    if verb == 'G' {
                        g.to_uppercase()
                    } else {
                        g
                    }
                }
                _ => format_float_g(arg_float(v), spec.prec, verb == 'G'),
            };
            signed(s, spec)
        }
        'd' => match arg_uint(v) {
            Some(u) if spec.plus => format!("+{u}"),
            Some(u) => u.to_string(),
            None => {
                let n = arg_int(v);
                if spec.plus && n >= 0 {
                    format!("+{n}")
                } else {
                    n.to_string()
                }
            }
        },
        // `%x`/`%X` hex-encode a string bytewise.
        'x' | 'X' if matches!(v, Value::Str(_)) => {
            let upper = verb == 'X';
            let body: String = go_str(v)
                .bytes()
                .map(|b| {
                    if upper {
                        format!("{b:02X}")
                    } else {
                        format!("{b:02x}")
                    }
                })
                .collect();
            if spec.sharp {
                format!("{}{body}", if upper { "0X" } else { "0x" })
            } else {
                body
            }
        }
        // The base-N verbs. Go prints a *signed* operand as a sign and the
        // magnitude — `%x` of `-9` is `-9`, not the two's-complement bit pattern
        // — and only an unsigned one reads all 64 bits. `#` writes the base
        // prefix after the sign: `-0x9`, `-0b1001`, and a leading `0` for octal
        // where the digits do not already start with one.
        'x' | 'X' | 'o' | 'b' => {
            let (neg, mag) = match arg_uint(v) {
                Some(u) => (false, u),
                None => {
                    let n = arg_int(v);
                    (n < 0, n.unsigned_abs())
                }
            };
            let digits = match verb {
                'x' => format!("{mag:x}"),
                'X' => format!("{mag:X}"),
                'o' => format!("{mag:o}"),
                _ => format!("{mag:b}"),
            };
            let prefix = match (spec.sharp, verb) {
                (false, _) => "",
                (true, 'x') => "0x",
                (true, 'X') => "0X",
                (true, 'b') => "0b",
                (true, _) if digits.starts_with('0') => "",
                (true, _) => "0",
            };
            let sign = if neg {
                "-"
            } else if spec.plus {
                "+"
            } else {
                ""
            };
            format!("{sign}{prefix}{digits}")
        }
        // A code point outside Unicode — a negative or too-large integer — is
        // the replacement character, as Go's rune conversion makes it.
        'c' => u32::try_from(arg_int(v))
            .ok()
            .and_then(char::from_u32)
            .unwrap_or('\u{fffd}')
            .to_string(),
        // `%U` is Go's Unicode format: `U+4E16`, at least four hex digits. It
        // reads the operand's bits unsigned, so a negative integer shows all 64
        // (`U+FFFFFFFFFFFFFFF7`) rather than clamping to zero.
        'U' => format!("U+{:04X}", v.to_int() as u64),
        // `%s` on a byte slice is handled by the caller; here it is the value's
        // own text, truncated by any precision.
        's' => {
            let mut s = match v {
                Value::Str(s) => s.as_str().to_string(),
                v => match bytes_of(v) {
                    Some(b) => String::from_utf8_lossy(&b).into_owned(),
                    None => go_str(v),
                },
            };
            if let Some(p) = spec.prec {
                if s.chars().count() > p {
                    s = s.chars().take(p).collect();
                }
            }
            s
        }
        _ => go_str(v),
    }
}

/// Go's bad-verb rendering — `%!q(bool=true)` — for the operand kinds a verb
/// does not accept, or `None` when the verb accepts this one.
///
/// Only the kinds go-rs's value model names the same way Go's static types do
/// are reported: a `Value::Str` is always a Go `string` and a `Value::Bool`
/// always a `bool`, but a `Value::Int` can be an untyped constant that Go widened
/// to a `float64` on the way into a `[]float64`, so `%f` of an integer stays a
/// coercion rather than becoming a (wrong) `%!f(int=…)`.
///
/// `%b` and `%x`/`%X` are absent from the float row on purpose: Go accepts a
/// float under both (`%x` of `1.5` is `0x1.8p+00`), so a float there is a
/// *valid* operand go-rs renders differently, not a bad verb.
///
/// The operand inside the parentheses carries the verb's own width and
/// precision — `%05d` of `"k"` is `%!d(string=0000k)` — so the bad form is
/// already padded and the caller must not pad it again.
fn bad_verb(v: &Value, verb: char, spec: &Spec, depth: usize) -> Option<String> {
    // A nil operand is the bad-verb form on its own; nested in a composite it is
    // just `<nil>`, which is what Go's `printValue` writes for an invalid entry.
    if matches!(v, Value::Undef) {
        return Some(if depth == 0 {
            format!("%!{verb}(<nil>)")
        } else {
            "<nil>".to_string()
        });
    }
    let bad = match verb {
        'd' | 'o' | 'c' | 'U' => matches!(v, Value::Str(_) | Value::Bool(_) | Value::Float(_)),
        'b' => matches!(v, Value::Str(_) | Value::Bool(_)),
        'x' | 'X' => matches!(v, Value::Bool(_)),
        'f' | 'F' | 'e' | 'E' | 'g' | 'G' => matches!(v, Value::Str(_) | Value::Bool(_)),
        'q' => matches!(v, Value::Bool(_) | Value::Float(_)),
        's' => matches!(v, Value::Bool(_) | Value::Int(_) | Value::Float(_)),
        't' => !matches!(v, Value::Bool(_)),
        _ => false,
    };
    if !bad {
        return None;
    }
    let mut text = go_str(v);
    if let Some(p) = spec.prec {
        if text.chars().count() > p {
            text = text.chars().take(p).collect();
        }
    }
    Some(format!(
        "%!{verb}({}={})",
        go_type_name(v),
        pad(&text, verb, spec)
    ))
}

/// Apply `+` to an unsigned float rendering.
fn signed(s: String, spec: &Spec) -> String {
    if spec.plus && !s.starts_with('-') {
        format!("+{s}")
    } else {
        s
    }
}

/// Pad one rendered value to the verb's width: right-justified by default, `-`
/// left, `0` zero-filling a numeric verb after any sign.
fn pad(body: &str, verb: char, spec: &Spec) -> String {
    let Some(width) = spec.width else {
        return body.to_string();
    };
    let len = body.chars().count();
    if len >= width {
        return body.to_string();
    }
    let fill = width - len;
    if spec.left {
        return format!("{body}{}", " ".repeat(fill));
    }
    if spec.zero && matches!(verb, 'd' | 'f' | 'F' | 'x' | 'X' | 'o' | 'b') {
        let (sign, rest) = match body.strip_prefix(['-', '+']) {
            Some(d) => (&body[..1], d),
            None => ("", body),
        };
        // A `#` base prefix sits between the sign and the zeros and does not
        // count toward the width at all: `%#08x` of `-9` is `-0x0000009`, whose
        // seven digits plus the sign make the eight. (Octal's prefix is a `0`,
        // which the fill supplies on its own.)
        let (prefix, digits) = match rest.get(..2) {
            Some(p @ ("0x" | "0X" | "0b")) => (p, &rest[2..]),
            _ => ("", rest),
        };
        return format!("{sign}{prefix}{}{digits}", "0".repeat(fill + prefix.len()));
    }
    format!("{}{body}", " ".repeat(fill))
}

/// A minimal `strings` and `strconv` standard library. Each exported function
/// is a numbered builtin the compiler dispatches `strings.X` / `strconv.X` calls
/// to. Split/Fields return heap slices; Join reads one.
pub mod stdlib {
    use super::{go_str, heap_alloc, pop_args, HostObj, HEAP};
    use fusevm::{Value, VM};
    use std::cell::RefCell;

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
    /// `strconv.ErrSyntax` / `strconv.ErrRange` — the sentinel by name.
    pub const STRCONV_ERR: u16 = 856;
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
    // math.* trigonometry / exponential / logarithm (added wave).
    pub const SIN: u16 = 907;
    pub const COS: u16 = 908;
    pub const TAN: u16 = 909;
    pub const ASIN: u16 = 910;
    pub const ACOS: u16 = 911;
    pub const ATAN: u16 = 912;
    pub const ATAN2: u16 = 913;
    pub const SINH: u16 = 914;
    pub const COSH: u16 = 915;
    pub const TANH: u16 = 916;
    pub const EXP: u16 = 917;
    pub const LOG: u16 = 918;
    pub const LOG2: u16 = 919;
    pub const LOG10: u16 = 920;
    pub const CBRT: u16 = 921;
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
            ("math", "Sin") => SIN,
            ("math", "Cos") => COS,
            ("math", "Tan") => TAN,
            ("math", "Asin") => ASIN,
            ("math", "Acos") => ACOS,
            ("math", "Atan") => ATAN,
            ("math", "Atan2") => ATAN2,
            ("math", "Sinh") => SINH,
            ("math", "Cosh") => COSH,
            ("math", "Tanh") => TANH,
            ("math", "Exp") => EXP,
            ("math", "Log") => LOG,
            ("math", "Log2") => LOG2,
            ("math", "Log10") => LOG10,
            ("math", "Cbrt") => CBRT,
            ("sort", "Ints") => SORT_INTS,
            ("sort", "Strings") => SORT_STRINGS,
            ("sort", "Float64s") => SORT_FLOAT64S,
            ("os", "Getenv") => GETENV,
            _ => return None,
        })
    }

    /// Whether `pkg.func` returns Go's `(value, error)` pair rather than a bare
    /// value — so the compiler destructures it and rejects its use where one
    /// value is expected.
    pub fn returns_error(pkg: &str, func: &str) -> bool {
        matches!(
            (pkg, func),
            ("strconv", "Atoi") | ("strconv", "ParseInt") | ("strconv", "ParseFloat")
        )
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
        vm.register_builtin(PARSE_FLOAT, b_parse_float);
        vm.register_builtin(FORMAT_INT, b_format_int);
        // `strconv.Quote` is the same double-quoted Go literal `%q` produces —
        // escapes and all, which wrapping the raw string in quotes was not.
        vm.register_builtin(QUOTE, |vm, a| s1(vm, a, super::go_quote));
        vm.register_builtin(STRCONV_ERR, b_strconv_err);
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
        vm.register_builtin(SIN, |vm, a| math1(vm, a, f64::sin));
        vm.register_builtin(COS, |vm, a| math1(vm, a, f64::cos));
        vm.register_builtin(TAN, |vm, a| math1(vm, a, f64::tan));
        vm.register_builtin(ASIN, |vm, a| math1(vm, a, f64::asin));
        vm.register_builtin(ACOS, |vm, a| math1(vm, a, f64::acos));
        vm.register_builtin(ATAN, |vm, a| math1(vm, a, f64::atan));
        vm.register_builtin(ATAN2, |vm, a| math2(vm, a, f64::atan2));
        vm.register_builtin(SINH, |vm, a| math1(vm, a, f64::sinh));
        vm.register_builtin(COSH, |vm, a| math1(vm, a, f64::cosh));
        vm.register_builtin(TANH, |vm, a| math1(vm, a, f64::tanh));
        vm.register_builtin(EXP, |vm, a| math1(vm, a, f64::exp));
        vm.register_builtin(LOG, |vm, a| math1(vm, a, f64::ln));
        vm.register_builtin(LOG2, |vm, a| math1(vm, a, f64::log2));
        vm.register_builtin(LOG10, |vm, a| math1(vm, a, f64::log10));
        vm.register_builtin(CBRT, |vm, a| math1(vm, a, f64::cbrt));
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
                    if let Some(HostObj::Slice { elems: a, .. }) =
                        h.borrow_mut().get_mut(backing as usize)
                    {
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

    /// `strconv.ParseInt(s, base, bitSize) (int64, error)`.
    fn b_parse_int(vm: &mut VM, argc: u8) -> Value {
        let args = pop_args(vm, argc);
        let s = args.first().map(go_str).unwrap_or_default();
        let base = args.get(1).map(|v| v.to_int()).unwrap_or(10).max(2) as u32;
        parse_signed(&s, base, "ParseInt")
    }

    /// `strconv.ParseFloat(s, bitSize) (float64, error)`. An overflowing literal
    /// yields ±Inf *and* a range error, as Go's does; a written `Inf` does not.
    fn b_parse_float(vm: &mut VM, argc: u8) -> Value {
        let args = pop_args(vm, argc);
        let s = args.first().map(go_str).unwrap_or_default();
        match s.parse::<f64>() {
            Ok(f)
                if f.is_infinite()
                    && !s.trim_start_matches(['+', '-']).eq_ignore_ascii_case("inf") =>
            {
                parsed(Value::Float(f), Some(num_error("ParseFloat", &s, RANGE)))
            }
            Ok(f) => parsed(Value::Float(f), None),
            Err(_) => parsed(Value::Float(0.0), Some(num_error("ParseFloat", &s, SYNTAX))),
        }
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
        Value::Obj(heap_alloc(HostObj::slice(parts)))
    }

    fn b_fields(vm: &mut VM, argc: u8) -> Value {
        let args = pop_args(vm, argc);
        let s = args.first().map(go_str).unwrap_or_default();
        let parts: Vec<Value> = s.split_whitespace().map(Value::str).collect();
        Value::Obj(heap_alloc(HostObj::slice(parts)))
    }

    fn b_join(vm: &mut VM, argc: u8) -> Value {
        let args = pop_args(vm, argc);
        let sep = args.get(1).map(go_str).unwrap_or_default();
        let joined = match args.first() {
            Some(Value::Obj(id)) => HEAP.with(|h| {
                let h = h.borrow();
                let elems: &[Value] = match h.get(*id as usize) {
                    Some(HostObj::Slice { elems: a, .. }) => a,
                    Some(HostObj::SliceView {
                        backing,
                        offset,
                        len,
                        ..
                    }) => match h.get(*backing as usize) {
                        Some(HostObj::Slice { elems: a, .. }) => &a[*offset..*offset + *len],
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

    /// A `(value, error)` result as the 2-element tuple the compiler
    /// destructures, with `err` nil on success.
    fn parsed(value: Value, err: Option<Value>) -> Value {
        let e = err.unwrap_or(Value::Undef);
        Value::Obj(heap_alloc(HostObj::slice(vec![value, e])))
    }

    thread_local! {
        /// The two `strconv` sentinel errors, allocated once per run so every
        /// `NumError.Err` and every `strconv.ErrSyntax` mention is the *same*
        /// pointer — which is what `errors.Is` compares. Cleared by
        /// [`super::heap_reset`] along with the heap the handles index.
        static SENTINELS: RefCell<std::collections::HashMap<&'static str, Value>> =
            RefCell::new(std::collections::HashMap::new());
    }

    pub(super) fn sentinels_reset() {
        SENTINELS.with(|s| s.borrow_mut().clear());
    }

    /// `strconv.ErrSyntax` / `strconv.ErrRange` — Go's `errors.New` sentinels,
    /// memoized so repeated mentions compare equal by pointer.
    fn sentinel(reason: &'static str) -> Value {
        SENTINELS.with(|s| {
            s.borrow_mut()
                .entry(reason)
                .or_insert_with(|| super::make_error(reason.to_string()))
                .clone()
        })
    }

    /// `strconv.ErrSyntax` / `strconv.ErrRange` read as a value (stack: name).
    fn b_strconv_err(vm: &mut VM, argc: u8) -> Value {
        let args = pop_args(vm, argc);
        match args.first().map(go_str).unwrap_or_default().as_str() {
            "ErrRange" => sentinel(RANGE),
            _ => sentinel(SYNTAX),
        }
    }

    /// The `*strconv.NumError` Go returns from a failed conversion: the function
    /// name, the input, and the `ErrSyntax`/`ErrRange` sentinel it wraps. Its
    /// `Error()` and `Unwrap()` are synthesized as Go source by `crate::pkg`, so
    /// the message text, `errors.Is` and `errors.As` all come off the real type.
    fn num_error(func: &str, num: &str, reason: &'static str) -> Value {
        Value::Obj(heap_alloc(HostObj::Struct {
            type_name: NUM_ERROR.to_string(),
            fields: vec![
                ("Func".to_string(), Value::str(func.to_string())),
                ("Num".to_string(), Value::str(num.to_string())),
                ("Err".to_string(), sentinel(reason)),
            ],
            by_ref: true,
        }))
    }

    /// The linked name of `strconv`'s `NumError` — `strconv` is a native package,
    /// so its one source-level type is synthesized under its qualified name.
    pub const NUM_ERROR: &str = "strconv.NumError";

    const SYNTAX: &str = "invalid syntax";
    const RANGE: &str = "value out of range";

    /// Parse a signed integer in `base` the way `strconv` does: no surrounding
    /// whitespace is allowed, and an out-of-range value saturates *and* reports a
    /// range error (Go returns the clamped value alongside it).
    fn parse_signed(s: &str, base: u32, func: &str) -> Value {
        match i64::from_str_radix(s, base) {
            Ok(n) => parsed(Value::Int(n), None),
            Err(e) => {
                let (v, reason) = match e.kind() {
                    std::num::IntErrorKind::PosOverflow => (i64::MAX, RANGE),
                    std::num::IntErrorKind::NegOverflow => (i64::MIN, RANGE),
                    _ => (0, SYNTAX),
                };
                parsed(Value::Int(v), Some(num_error(func, s, reason)))
            }
        }
    }

    /// `strconv.Atoi(s) (int, error)` — a base-10 signed integer.
    fn b_atoi(vm: &mut VM, argc: u8) -> Value {
        let args = pop_args(vm, argc);
        let s = args.first().map(go_str).unwrap_or_default();
        parse_signed(&s, 10, "Atoi")
    }
}

/// Both operands as `i64`, or `None` if either is not an integer.
fn int_pair(a: &Value, b: &Value) -> Option<(i64, i64)> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Some((*x, *y)),
        _ => None,
    }
}

/// One operand an `Int` and the other a `Float`. Go's type system never builds
/// this pair out of a concrete expression — it has no implicit numeric
/// conversion — so it only arrives from an interface comparison. See
/// [`numeric_hook`].
fn mixed_num_pair(a: &Value, b: &Value) -> bool {
    matches!(
        (a, b),
        (Value::Int(_), Value::Float(_)) | (Value::Float(_), Value::Int(_))
    )
}

/// Strict numeric hook. fusevm delegates an arithmetic or comparison op here
/// when it cannot answer it natively, which happens for four reasons:
///
/// 1. An operand is non-numeric — a string or a heap object. This is Go's `+`
///    string-concatenation overload and its string ordering; every other
///    arithmetic op on a string is a type error, reported rather than coerced
///    (Go rejects `"a" - 1`).
/// 2. An all-numeric *integer* pair whose native op overflowed. Go's integers
///    are fixed-width and wrap, so these are re-done with `wrapping_*`.
/// 3. An operand is nil (`Undef`) — the erased zero value of a generic type
///    parameter, or a typed nil being compared for identity.
/// 4. A mixed `Int`/`Float` pair whose integer is past 2^53, where promoting it
///    to `f64` would round to a neighbouring value.
///
/// So "all-numeric never reaches here" is false in both directions: cases 2 and
/// 4 are all-numeric, and case 1 is the only one involving a string.
pub fn numeric_hook(op: NumOp, a: &Value, b: &Value) -> Result<Value, String> {
    match op {
        // The zero value of an erased generic type parameter (`var total T`) is
        // nil (`Undef`); Go would use T's concrete zero. Treat nil as the
        // additive identity so a generic accumulator matches Go for whichever
        // concrete type is passed: nil+int→int, nil+float→float, nil+str→str.
        NumOp::Add if matches!(a, Value::Undef) => Ok(b.clone()),
        NumOp::Add if matches!(b, Value::Undef) => Ok(a.clone()),
        // fusevm also routes here when a native integer op overflows. Go's
        // integers are fixed-width and wrap on overflow (two's complement), so
        // two integer operands wrap instead of reaching the string branches
        // below — which used to turn `int64max + 1` into the *concatenation*
        // "92233720368547758071" rather than -9223372036854775808.
        NumOp::Add | NumOp::Sub | NumOp::Mul if int_pair(a, b).is_some() => {
            let (x, y) = int_pair(a, b).unwrap_or((0, 0));
            Ok(Value::Int(match op {
                NumOp::Add => x.wrapping_add(y),
                NumOp::Sub => x.wrapping_sub(y),
                _ => x.wrapping_mul(y),
            }))
        }
        NumOp::Neg if matches!(a, Value::Int(_)) => Ok(Value::Int(a.to_int().wrapping_neg())),
        // fusevm answers a mixed Int/Float pair natively by promoting the
        // integer to f64, but past 2^53 that promotion lands on a
        // *neighbouring* value (3^34 = 16_677_181_699_666_569 has the f64
        // image …568), so it hands the pair over rather than round it.
        //
        // Answering does not need the exact integer. Go has no implicit
        // numeric conversion, so a mixed pair is unrepresentable in a concrete
        // expression: with `var i int64; var f float64`, both `i + f` and
        // `i == f` are compile errors ("mismatched types int64 and float64").
        // The one construct in valid Go that puts an int beside a float64
        // under a single operator is comparing two interfaces —
        // `any(1) == any(1.0)` — and interface equality is decided by dynamic
        // type before value: different types are never equal, whatever the
        // numbers are. Go prints `false` there, and would print `false` just
        // the same for `any(int64(1e18)) == any(float64(1e18))`. The rounding
        // fusevm was worried about cannot change the answer.
        //
        // That comparison no longer arrives here: the compiler routes an `==`
        // with an interface operand to [`GIFACE_EQ`], because a *small* mixed
        // pair is answered inside fusevm by promoting the integer exactly and
        // so never reaches the frontend at all. This arm stays as the backstop
        // for a mixed pair that reaches an ordinary comparison some other way,
        // and it answers the same way [`iface_eq`] does.
        NumOp::Eq | NumOp::Ne if mixed_num_pair(a, b) => Ok(Value::bool(op == NumOp::Ne)),
        // Every other operator on a mixed pair is unreachable from valid Go:
        // arithmetic needs an explicit conversion, and interfaces are
        // unordered (`a < b` on two `any` is "operator < not defined on
        // interface"). There is nothing to promote, so report it the way Go's
        // type checker does instead of falling through to the string branches
        // below — which would concatenate `+` into "166771816996665690.5" and
        // order `<` lexicographically, both silently.
        NumOp::Add
        | NumOp::Sub
        | NumOp::Mul
        | NumOp::Div
        | NumOp::Mod
        | NumOp::Pow
        | NumOp::Lt
        | NumOp::Gt
        | NumOp::Le
        | NumOp::Ge
            if mixed_num_pair(a, b) =>
        {
            let (x, y) = if matches!(a, Value::Int(_)) {
                ("int", "float64")
            } else {
                ("float64", "int")
            };
            Err(format!(
                "go-rs: invalid operation: operator {op:?} not defined on mismatched types {x} and {y}"
            ))
        }
        NumOp::Add => Ok(Value::str(format!("{}{}", go_str(a), go_str(b)))),
        // A typed nil (a nil slice or map) equals `nil` and nothing else — Go
        // permits no other comparison for those types.
        NumOp::Eq | NumOp::Ne if nil_kind(a).is_some() || nil_kind(b).is_some() => {
            let same = matches!((a, b), (Value::Undef, _) | (_, Value::Undef))
                || matches!((a, b), (Value::Obj(x), Value::Obj(y)) if x == y);
            Ok(Value::bool(if op == NumOp::Eq { same } else { !same }))
        }
        // A pointer compares by address, a struct value field by field.
        NumOp::Eq | NumOp::Ne if ptr_eq(a, b).is_some() => {
            let same = ptr_eq(a, b).unwrap_or(false);
            Ok(Value::bool(if op == NumOp::Eq { same } else { !same }))
        }
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
