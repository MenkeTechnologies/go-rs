//! Language Server Protocol over stdio (`go --lsp`).
//!
//! Self-contained and read-only: diagnostics come from the same `parser::parse`
//! the runtime uses (a syntax error maps to the reported line); hover and
//! completion draw on the reference corpus below. No output ever reaches the
//! terminal — JSON-RPC on stdio only. Structure follows the sibling `-rs`
//! frontends' `lsp.rs`.

use std::collections::HashMap;

use lsp_server::{Connection, ErrorCode, ExtractError, Message, Request, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
    PublishDiagnostics,
};
use lsp_types::request::{Completion, HoverRequest, Request as _};
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse,
    Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, Hover, HoverContents, HoverParams, HoverProviderCapability,
    MarkupContent, MarkupKind, Position, PublishDiagnosticsParams, Range, ServerCapabilities,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions, Uri,
};

/// One documented surface of the language.
pub struct Entry {
    /// The name as it is written in Go source (`append`, `strings.Join`, `%d`).
    pub name: &'static str,
    /// The chapter it belongs to; also the grouping `go doc` and
    /// `docs/reference.html` render.
    pub chapter: &'static str,
    /// The declaration form — a call signature, a type form, or the syntax of
    /// the construct.
    pub signature: &'static str,
    /// What the current go-rs build actually does, including where that differs
    /// from Go.
    pub doc: &'static str,
    /// A short, runnable illustration.
    pub example: &'static str,
}

/// Terse constructor so the corpus table below reads as data, not boilerplate.
const fn e(
    name: &'static str,
    chapter: &'static str,
    signature: &'static str,
    doc: &'static str,
    example: &'static str,
) -> Entry {
    Entry {
        name,
        chapter,
        signature,
        doc,
        example,
    }
}

/// The reference corpus. Single source of truth for LSP completion and hover,
/// for `go doc`, and for the offline `docs/reference.html` generator. Every
/// entry mirrors a surface the current go-rs build actually implements:
///   * "Keyword" → a reserved word in `lexer.rs` (`keyword_or_ident`).
///   * "Type" / "Conversion" → `compiler.rs` (`numtype_of_ty`,
///     `is_conversion_type`) and the `GCONV` builtin in `host.rs`.
///   * "Builtin" → `compiler.rs` (`is_builtin_call` and the
///     special-cased names) lowered to the `G*` builtins in `host.rs`.
///   * "Package …" → the native dispatch tables (`host::stdlib::resolve`,
///     `host::stdlib::resolve_const`, the `fmt` arm of `Compiler::call`) or the
///     Go source vendored in `goroot/` and linked by `pkg.rs`.
///   * "Operator" / "Statement" / "Expression" → the token set in `lexer.rs`
///     and the `Stmt` / `Expr` / `BinOp` / `UnOp` / `AssignOp` variants in
///     `ast.rs`.
///   * "Divergence from Go" → behaviour that differs from gc-compiled Go,
///     each verified by running the current build.
const CORPUS: &[Entry] = &[
    // ── Keyword (lexer.rs `keyword_or_ident`) ──
    e(
        "package",
        "Keyword",
        "package NAME",
        "Names the compilation unit's package. go-rs runs `func main` of `package main`; for an imported source package the clause names the package whose top-level identifiers the linker qualifies before merging them into the one compile unit.",
        "package main",
    ),
    e(
        "import",
        "Keyword",
        "import \"PATH\"   |   import ( \"PATH\" … )",
        "Names a package to link. `fmt`, `strings`, `strconv`, `math`, `sort` and `os` are native host builtins and are never loaded from source; every other path is resolved to real Go source (vendored, installed under ~/.go-rs, or a local GOROOT) and merged into the same program.",
        "import (\n\t\"fmt\"\n\t\"strings\"\n)",
    ),
    e(
        "func",
        "Keyword",
        "func NAME(params) results { … }",
        "Declares a function, a method (with a receiver), or opens a function literal. `func main` is compiled into the chunk's global scope; every other func becomes a fusevm subroutine whose locals live in call-frame slots, so recursion never clobbers a caller.",
        "func add(a int, b int) int { return a + b }",
    ),
    e(
        "var",
        "Keyword",
        "var NAME [T] [= expr]",
        "Declares a variable. With no initializer it takes the zero value of its type: `0.0` for a float type, `\"\"` for string, `false` for bool, and `0` for everything else — including pointers, slices and maps, which Go would leave nil.",
        "var n int = 42",
    ),
    e(
        "const",
        "Keyword",
        "const NAME [T] = expr   |   const ( … )",
        "Declares a constant. go-rs models it as an immutable-by-convention variable evaluated at its declaration site; a grouped block supports `iota` and Go's rule that a spec with no expression repeats the previous one.",
        "const (\n\tA = iota\n\tB\n\tC\n)",
    ),
    e(
        "type",
        "Keyword",
        "type NAME struct { … }   |   type NAME interface { … }",
        "Declares a named struct or interface. Only top-level declarations are parsed — a `type` declaration inside a function body is a syntax error.",
        "type Point struct { x, y int }",
    ),
    e(
        "struct",
        "Keyword",
        "struct { field T; … }",
        "A struct type: a fixed set of named fields with Go value semantics. The compiler emits a struct copy on assignment, parameter binding and return, so a struct handle is never aliased. Anonymous (embedded) fields are not parsed.",
        "type Point struct { x, y int }",
    ),
    e(
        "interface",
        "Keyword",
        "interface { Method(…) T; … }",
        "An interface type, written as a method set inside a `type` declaration. There is no method table: the compiler generates a runtime type switch over every concrete type that carries the method. `interface{}` is not accepted where a type is expected in a `var` or parameter — write `any`.",
        "type Shape interface { Area() int }",
    ),
    e(
        "if",
        "Keyword",
        "if [init;] cond { … } [else …]",
        "Conditional branch with an optional init statement whose bindings are scoped to the branch. Lowers to `JumpIfFalse` over the then-block.",
        "if v, err := f(); err != nil { return err }",
    ),
    e(
        "else",
        "Keyword",
        "else { … }   |   else if cond { … }",
        "The fallback branch of an `if`. Chains as `else if`, which the parser nests as an `if` inside the else block.",
        "if x > 0 { pos() } else { nonpos() }",
    ),
    e(
        "for",
        "Keyword",
        "for [init;] [cond;] [post] { … }",
        "The only loop keyword. Covers the three-clause, condition-only and infinite forms; `for … range` is the fourth. `break` and `continue` target the innermost loop.",
        "for i := 0; i < 3; i++ { fmt.Println(i) }",
    ),
    e(
        "range",
        "Keyword",
        "for [k[, v]] := range iter { … }",
        "Iterates a slice, array, map or string. A string yields the byte offset and the code point of each rune. A map yields its keys in insertion order — go-rs does not randomize. Ranging a channel or an integer yields zero iterations.",
        "for i, c := range \"hé\" { fmt.Println(i, c) }",
    ),
    e(
        "return",
        "Keyword",
        "return [expr[, expr …]]",
        "Returns from the current function with zero, one, or several results. Multiple results are packed into a slice the caller destructures. With named results, a bare `return` yields their current values.",
        "return a + b, nil",
    ),
    e(
        "break",
        "Keyword",
        "break",
        "Leaves the innermost enclosing `for` loop or `switch`. Labelled break is not parsed.",
        "for { if done { break } }",
    ),
    e(
        "continue",
        "Keyword",
        "continue",
        "Starts the next iteration of the innermost enclosing loop. A `continue` written inside a `switch` reaches the loop around it, because switch scopes catch `break` only.",
        "if i%2 == 0 { continue }",
    ),
    e(
        "true",
        "Keyword",
        "true",
        "The boolean literal true. It is a distinct token rather than a predeclared identifier, so unlike Go it cannot be shadowed by a variable of the same name.",
        "ok := true",
    ),
    e(
        "false",
        "Keyword",
        "false",
        "The boolean literal false. Like `true`, a reserved token rather than a predeclared identifier.",
        "ok := false",
    ),
    e(
        "go",
        "Keyword",
        "go f(args)",
        "Spawns a goroutine: `f` and its arguments are evaluated now, then the call runs on fusevm's cooperative scheduler. Lowers to `Op::Go`, one of the six concurrency opcodes go-rs uses from fusevm 0.15.",
        "go worker(id, results)",
    ),
    e(
        "chan",
        "Keyword",
        "chan T",
        "A channel type. `make(chan T, n)` builds one with buffer capacity `n`; omitting `n` gives an unbuffered channel. The element type is erased — every channel carries dynamic values. Directional forms (`<-chan T`, `chan<- T`) are not parsed.",
        "ch := make(chan int, 8)",
    ),
    e(
        "select",
        "Keyword",
        "select { case …: …; default: … }",
        "Waits on several channel operations at once. A ready case runs; otherwise `default` runs when present, else the goroutine blocks. Lowers to `Op::Select`, which pushes the received value and the index of the case that fired.",
        "select {\ncase v := <-ch:\n\tuse(v)\ndefault:\n}",
    ),
    e(
        "switch",
        "Keyword",
        "switch [init;] [tag] { case …: … }",
        "Multi-way branch in either the tagged form (`switch x`, each case compared to `x`) or the expression form (`switch`, each case a boolean). The first match runs and then leaves the switch — there is no implicit fallthrough. `case a, b:` matches either.",
        "switch {\ncase n < 0:\n\tneg()\ndefault:\n\tpos()\n}",
    ),
    e(
        "fallthrough",
        "Keyword",
        "fallthrough",
        "Transfers control into the next case's body without evaluating its expression. Must be the last statement of its case.",
        "case 2:\n\tp()\n\tfallthrough\ncase 3:\n\tq()",
    ),
    e(
        "defer",
        "Keyword",
        "defer f(args)",
        "Schedules a call to run when the enclosing function returns, in LIFO order. `f` and its arguments are evaluated at the `defer` statement, not at return. A deferred function literal is where `recover` can stop a panic.",
        "defer func() { recover() }()",
    ),
    // ── Predeclared identifier (parser / compiler, not lexer keywords) ──
    e(
        "map",
        "Predeclared Identifier",
        "map[K]V",
        "The map type constructor — a predeclared identifier, not a keyword. A go-rs map is an association list: lookup, insert and delete are a linear scan, keys compare by value (structs field by field), and iteration order is insertion order.",
        "m := make(map[string]int)",
    ),
    e(
        "any",
        "Predeclared Identifier",
        "any",
        "The empty interface, accepted anywhere a type is expected. Because the value model is dynamic, `any` erases to no constraint at all. Use it in place of `interface{}`, which the type parser does not accept in that position.",
        "var v any = \"s\"",
    ),
    e(
        "error",
        "Predeclared Identifier",
        "error",
        "The error interface. In go-rs it is a type name whose zero value is nil; any value with an `Error() string` method satisfies it, and `fmt` prints such a value through that method via the synthesized `$stringify` helper.",
        "func f() (int, error) { return 0, nil }",
    ),
    e(
        "nil",
        "Predeclared Identifier",
        "nil",
        "The absent value — fusevm's `Undef`. It is what an unset result, a missing struct handle, or a zero-valued type parameter holds, which is what makes the `err != nil` idiom work. Adding nil to a number or string is the identity, so a generic accumulator starting at nil still totals correctly.",
        "if err != nil { return err }",
    ),
    e(
        "iota",
        "Predeclared Identifier",
        "iota",
        "Inside a grouped `const` block, the zero-based index of the current specification. The parser substitutes the integer literal directly into the constant expression, so `1 << iota` and similar folded forms work.",
        "const ( KB = 1 << (10 * (iota + 1)) )",
    ),
    e(
        "_",
        "Predeclared Identifier",
        "_",
        "The blank identifier. As an assignment or short-declaration target the value is evaluated and discarded; as a `range` key or value the binding is skipped entirely.",
        "_, err := f()",
    ),
    e(
        "main",
        "Predeclared Identifier",
        "func main()",
        "The entry point. Its body is compiled into the chunk's global scope, so its variables are addressed by name (`GetVar`/`SetVar`) rather than by frame slot; every other function's locals are slot-addressed.",
        "func main() { fmt.Println(\"hi\") }",
    ),
    e(
        "init",
        "Predeclared Identifier",
        "func init()",
        "Go runs every `init` function before `main`. go-rs parses `func init` as an ordinary function and never calls it — its body runs only if something calls it by name. Package-level `var` initializers, by contrast, do run first, in dependency-load order.",
        "func init() { … }   // never runs on its own",
    ),
    // ── Type (compiler.rs `numtype_of_ty` + the type-name grammar) ──
    e(
        "int",
        "Type",
        "int",
        "Machine integer, stored in an i64. The static type is what makes `int / int` truncate toward zero (the compiler appends `Op::TruncInt`) and what selects the numeric rather than string comparison ops.",
        "var n int = 7",
    ),
    e(
        "int8",
        "Type",
        "int8",
        "8-bit signed integer. The annotation only classifies the value as an integer for division and comparison; the value still occupies a full i64. An explicit `int8(x)` conversion is what actually wraps to 8 bits.",
        "var b int8 = -128",
    ),
    e(
        "int16",
        "Type",
        "int16",
        "16-bit signed integer. Annotation-only, exactly like `int8`; arithmetic does not wrap at 16 bits unless converted.",
        "var s int16 = 32767",
    ),
    e(
        "int32",
        "Type",
        "int32",
        "32-bit signed integer, and the underlying type of `rune`. Annotation-only in arithmetic.",
        "var r int32 = 'A'",
    ),
    e(
        "int64",
        "Type",
        "int64",
        "64-bit signed integer — go-rs's native integer width, so `int64(x)` on an integer is the identity and no wrapping ever occurs.",
        "var big int64 = 9000000000",
    ),
    e(
        "uint",
        "Type",
        "uint",
        "Unsigned machine integer. go-rs stores it in the same signed i64 and does not mask, so a value with the top bit set reads back negative in arithmetic and printing.",
        "var u uint = 42",
    ),
    e(
        "uint8",
        "Type",
        "uint8",
        "8-bit unsigned integer, and the underlying type of `byte`. Conversion masks to the low 8 bits and zero-extends.",
        "var b uint8 = 255",
    ),
    e(
        "uint16",
        "Type",
        "uint16",
        "16-bit unsigned integer. Conversion masks to the low 16 bits and zero-extends.",
        "var s uint16 = 65535",
    ),
    e(
        "uint32",
        "Type",
        "uint32",
        "32-bit unsigned integer. Conversion masks to the low 32 bits and zero-extends.",
        "var u uint32 = 4294967295",
    ),
    e(
        "uint64",
        "Type",
        "uint64",
        "64-bit unsigned integer, held in an i64. A literal above i64::MAX (`0x8080808080808080`) is accepted and reinterpreted as the i64 with the same bit pattern, so it prints negative.",
        "var u uint64 = 1 << 63",
    ),
    e(
        "uintptr",
        "Type",
        "uintptr",
        "An integer wide enough to hold a pointer. Accepted as a type and a conversion for source compatibility; go-rs values are heap handles, not addresses, so it behaves as `int64`.",
        "var p uintptr = 0",
    ),
    e(
        "byte",
        "Type",
        "byte",
        "Alias for `uint8`. Indexing a string yields a byte, `len` counts bytes, and `[]byte(s)` yields the UTF-8 bytes as a slice of integers.",
        "var b byte = s[0]",
    ),
    e(
        "rune",
        "Type",
        "rune",
        "Alias for `int32`, holding a Unicode code point. A rune literal (`'A'`) is an integer, `string(r)` encodes it as UTF-8, and `range` over a string yields runes rather than bytes.",
        "for _, r := range s { fmt.Println(string(r)) }",
    ),
    e(
        "float32",
        "Type",
        "float32",
        "32-bit float. Every operation on a float32 is performed at 32-bit width (not rounded afterwards, which would round twice), and fmt prints the shortest decimal that round-trips at 32 bits: 1.0/3.0 is 0.33333334, not 0.3333333333333333.",
        "var f float32 = 1.5",
    ),
    e(
        "float64",
        "Type",
        "float64",
        "64-bit float — go-rs's native float. `fmt` prints a whole float with no fractional part (`3`, not `3.0`), matching Go's `%v`. Float constant expressions are folded with exact decimal arithmetic, so `0.1 + 0.2` prints `0.3`.",
        "var d float64 = 3.0   // prints 3",
    ),
    e(
        "string",
        "Type",
        "string",
        "Immutable UTF-8 string. `+` concatenates and the six comparison operators order lexicographically, all dispatched at runtime through the host's numeric hook. `len` and indexing are byte-based; `range` is rune-based.",
        "s := \"go\" + \"-rs\"",
    ),
    e(
        "bool",
        "Type",
        "bool",
        "Boolean. `&&` and `||` short-circuit through fusevm's keep-jumps (`JumpIfFalseKeep` / `JumpIfTrueKeep`), so the operand value itself is left on the stack.",
        "ok := 3 < 5",
    ),
    e(
        "[]T",
        "Type",
        "[]T",
        "Slice type: a handle to a backing array. Slices are reference types — assigning one shares the backing, and a sub-slice is a view into the same array so element writes are visible both ways.",
        "xs := []int{1, 2, 3}",
    ),
    e(
        "[N]T",
        "Type",
        "[N]T   |   [...]T",
        "Fixed-size array — a value type, unlike the slice `[]T`. It is copied elementwise on assign, argument bind, return, container store and read, `append`, channel send and `range`, so a write through a copy is invisible to the original at every depth; slice, map and pointer elements stay shared. `==` compares elementwise, which is what makes an array usable as a map key. `a[:]` yields a slice over the array's storage.",
        "var a [3]int",
    ),
    e(
        "map[K]V",
        "Type",
        "map[K]V",
        "Map type, represented as an association list of key/value pairs. Lookup is linear in the map's size and keys compare by value, which is what lets a struct or array key work.",
        "m := map[string]int{\"a\": 1}",
    ),
    e(
        "chan T",
        "Type",
        "chan T",
        "Channel type backed by fusevm's cooperative scheduler. The element type is parsed and discarded; a channel is a scheduler-owned id, not a heap object, which is why it cannot be ranged.",
        "ch := make(chan int)",
    ),
    e(
        "*T",
        "Type",
        "*T",
        "Pointer type. go-rs composite values are already heap handles, so a pointer is that same handle: `&x` copies nothing and `*p` is the identity. There is no pointer arithmetic and no null dereference distinct from nil.",
        "p := &Point{x: 1}",
    ),
    e(
        "func(…)",
        "Type",
        "func(params) results",
        "Function type, used for function-typed parameters, fields and variables. Its parameter and result types are consumed structurally and erased — every function value is a single closure handle, dispatched through `Op::CallDynamic`.",
        "func apply(f func(int) int, n int) int { return f(n) }",
    ),
    e(
        "struct{…}",
        "Type",
        "struct { field T; … }",
        "Anonymous struct type, usable as a field type, a map value type, or a channel element type. The parser assigns it a canonical generated name so the rest of the pipeline treats it as a declared type.",
        "s := struct{ n int }{n: 3}",
    ),
    e(
        "interface{…}",
        "Type",
        "interface { M() T; … }   |   interface{ ~int | ~float64 }",
        "Interface type in a `type` declaration; also the shape of a generic constraint, whose `~` underlying-type markers and `|` unions are parsed and erased.",
        "type Number interface{ ~int | ~float64 }",
    ),
    e(
        "T",
        "Type",
        "T",
        "A named type from a top-level `type` declaration. go-rs carries types as strings, so the name is what method dispatch, type switches and type assertions key on at runtime.",
        "type Point struct{ x, y int }",
    ),
    e(
        "pkg.T",
        "Type",
        "pkg.T",
        "A type from an imported source package. The linker rewrites the reference to the merged, qualified name so several packages can share one compile unit without colliding.",
        "var e errors.errorString",
    ),
    e(
        "T[…]",
        "Type",
        "T[K, V]",
        "A generic type reference. Type arguments are erased: `Stack[int]` types as `Stack`, and a type parameter's zero value is nil rather than the concrete type's zero.",
        "func Sum[T Number](xs []T) T",
    ),
    // ── Conversion (compiler.rs `is_conversion_type` → host.rs `b_conv`) ──
    e(
        "int(v)",
        "Conversion",
        "int(v) int",
        "Converts to the machine integer. A float truncates toward zero; a bool converts through its integer value. A string is not parsed — use `strconv.Atoi`.",
        "int(3.9)   // 3",
    ),
    e(
        "int8(v)",
        "Conversion",
        "int8(v) int8",
        "Truncates to the low 8 bits and sign-extends, so `int8(200)` is -56.",
        "int8(200)   // -56",
    ),
    e(
        "int16(v)",
        "Conversion",
        "int16(v) int16",
        "Truncates to the low 16 bits and sign-extends.",
        "int16(70000)   // 4464",
    ),
    e(
        "int32(v)",
        "Conversion",
        "int32(v) int32",
        "Truncates to the low 32 bits and sign-extends. The same conversion as `rune(v)`.",
        "int32(1 << 40)   // 0",
    ),
    e(
        "int64(v)",
        "Conversion",
        "int64(v) int64",
        "The identity on an integer, since go-rs stores every integer in an i64. A float still truncates toward zero.",
        "int64(3.7)   // 3",
    ),
    e(
        "uint(v)",
        "Conversion",
        "uint(v) uint",
        "Reinterprets as an unsigned machine integer without masking — the i64 bit pattern is kept, so a negative input stays negative rather than wrapping to a large positive as in Go.",
        "uint(5)",
    ),
    e(
        "uint8(v)",
        "Conversion",
        "uint8(v) uint8",
        "Masks to the low 8 bits and zero-extends.",
        "uint8(300)   // 44",
    ),
    e(
        "uint16(v)",
        "Conversion",
        "uint16(v) uint16",
        "Masks to the low 16 bits and zero-extends.",
        "uint16(70000)   // 4464",
    ),
    e(
        "uint32(v)",
        "Conversion",
        "uint32(v) uint32",
        "Masks to the low 32 bits and zero-extends.",
        "uint32(-1)   // 4294967295",
    ),
    e(
        "uint64(v)",
        "Conversion",
        "uint64(v) uint64",
        "The identity on the i64 bit pattern. Unlike Go, a negative input does not become a large positive value.",
        "uint64(7)",
    ),
    e(
        "uintptr(v)",
        "Conversion",
        "uintptr(v) uintptr",
        "The identity on the i64 bit pattern, accepted for source compatibility. go-rs has no addresses to hold.",
        "uintptr(0)",
    ),
    e(
        "byte(v)",
        "Conversion",
        "byte(v) byte",
        "Alias for `uint8(v)`: masks to the low 8 bits.",
        "byte(65)   // 65",
    ),
    e(
        "rune(v)",
        "Conversion",
        "rune(v) rune",
        "Alias for `int32(v)`: truncates to the low 32 bits. Paired with `string(r)` this is how a code point round-trips through an integer.",
        "string(rune(97))   // \"a\"",
    ),
    e(
        "float32(v)",
        "Conversion",
        "float32(v) float32",
        "Rounds through a real f32 and widens back to f64, reproducing the narrower type's precision loss.",
        "float32(0.1)",
    ),
    e(
        "float64(v)",
        "Conversion",
        "float64(v) float64",
        "Widens an integer or bool to a float; the identity on a float. This is how integer division is avoided: `float64(a) / float64(b)`.",
        "float64(7) / 2   // 3.5",
    ),
    e(
        "string(v)",
        "Conversion",
        "string(v) string",
        "From an integer, the UTF-8 encoding of that code point (an invalid one becomes U+FFFD). From a slice, the elements decoded as UTF-8 bytes when every element is a byte forming a valid sequence, and joined as code points otherwise — go-rs erases the element type, so it has to disambiguate `[]byte` from `[]rune` by inspection.",
        "string(65)   // \"A\"",
    ),
    e(
        "bool(v)",
        "Conversion",
        "bool(v) bool",
        "The truthiness of the value. Go has no such conversion; go-rs accepts it because the value model is dynamic.",
        "bool(1)   // true",
    ),
    e(
        "[]byte(s)",
        "Conversion",
        "[]byte(s) []byte",
        "The string's UTF-8 bytes as a slice of integers. A non-string argument passes through unchanged rather than erroring.",
        "[]byte(\"hé\")   // [104 195 169]",
    ),
    e(
        "[]rune(s)",
        "Conversion",
        "[]rune(s) []rune",
        "The string's Unicode code points as a slice of integers, one element per rune.",
        "[]rune(\"hé\")   // [104 233]",
    ),
    // ── Builtin (compiler.rs `is_builtin_call` + special cases) ──
    e(
        "len",
        "Builtin",
        "len(v) int",
        "The number of elements in a slice or map, or the number of bytes — not runes — in a string. Anything else is 0.",
        "len(\"héllo\")   // 6",
    ),
    e(
        "cap",
        "Builtin",
        "cap(v) int",
        "A slice's capacity: the backing array's length minus the view's offset, so a sub-slice can grow into the remaining backing without reallocating. `cap` of a string is its byte length; of a map, 0.",
        "cap(make([]int, 4))   // 4",
    ),
    e(
        "append",
        "Builtin",
        "append(s []T, elems …T) []T",
        "Extends `s` and returns the result. A plain slice is extended in place and the same handle comes back, so the growth is visible through every alias — Go detaches the result on reallocation, go-rs does not. A sub-slice view writes into the shared backing's spare capacity when it fits and reallocates into a fresh slice when it does not, matching Go. A nil first argument allocates.",
        "xs = append(xs, 4, 5)",
    ),
    e(
        "append",
        "Builtin",
        "append(s []T, other …T) []T",
        "The spread form. Every element of the base slice and of each spread slice is copied into a fresh backing array, so unlike the plain form this never mutates the base slice in place.",
        "xs = append(xs, ys...)",
    ),
    e(
        "copy",
        "Builtin",
        "copy(dst, src []T) int",
        "Copies min(len(dst), len(src)) elements into `dst` and returns the count. `src` may be a string, in which case its UTF-8 bytes are copied — the `copy([]byte, string)` form.",
        "n := copy(dst, src)",
    ),
    e(
        "delete",
        "Builtin",
        "delete(m map[K]V, k K)",
        "Removes key `k` from `m`; a no-op when absent. The key is located by value, so a struct key matches field by field rather than by handle identity.",
        "delete(m, \"a\")",
    ),
    e(
        "make",
        "Builtin",
        "make([]T, n [, cap]) []T",
        "Allocates a slice of `n` elements set to `T`'s zero value. The capacity argument is parsed and ignored — the capacity is always the length. A negative length raises a runtime fault.",
        "xs := make([]int, 3)",
    ),
    e(
        "make",
        "Builtin",
        "make(map[K]V) map[K]V",
        "Allocates an empty map. Go's optional size hint is not accepted here.",
        "m := make(map[string]int)",
    ),
    e(
        "make",
        "Builtin",
        "make(chan T [, n]) chan T",
        "Allocates a channel with buffer capacity `n`, or unbuffered when `n` is omitted. Lowers to `Op::ChanMake`, so the channel lives in the scheduler rather than on the host heap.",
        "ch := make(chan int, 8)",
    ),
    e(
        "new",
        "Builtin",
        "new(T) *T",
        "Allocates a zero value of `T` and returns a handle to it — the empty composite literal `&T{}` for a declared struct, and the address of the type's zero for a basic type. Since a go-rs pointer is the value's handle, `new(T)` and `&T{}` produce the same thing.",
        "p := new(Point)",
    ),
    e(
        "min",
        "Builtin",
        "min(x, y …T) T",
        "The smallest argument. When every argument is a string they compare lexicographically; otherwise comparison is numeric and the winning argument is returned unchanged, so an int input yields an int rather than a float.",
        "min(3, 1, 2)   // 1",
    ),
    e(
        "max",
        "Builtin",
        "max(x, y …T) T",
        "The largest argument, with the same string/numeric rule as `min`.",
        "max(3, 1, 2)   // 3",
    ),
    e(
        "panic",
        "Builtin",
        "panic(v any)",
        "Records `v` as the in-flight panic and jumps to the function's deferred-call drain, then unwinds to the caller's. Uncaught, the program prints `panic: v` on stderr and exits non-zero.",
        "panic(\"unreachable\")",
    ),
    e(
        "recover",
        "Builtin",
        "recover() any",
        "Inside a deferred call, stops the in-flight panic and returns its value; nil when no panic is active. Outside a deferred call it always returns nil.",
        "defer func() {\n\tif r := recover(); r != nil { fmt.Println(r) }\n}()",
    ),
    e(
        "close",
        "Builtin",
        "close(ch chan T)",
        "Closes a channel through `Op::ChanClose`; further receives yield the zero value. Note that closing does not make `for v := range ch` iterate — that form yields zero iterations either way.",
        "close(done)",
    ),
    e(
        "println",
        "Builtin",
        "println(args …any)",
        "The predeclared debug print: operands space-separated with a trailing newline, written to stderr. Distinct from `fmt.Println`, which writes stdout.",
        "println(\"debug\", x)",
    ),
    e(
        "print",
        "Builtin",
        "print(args …any)",
        "The predeclared debug print with no trailing newline, written to stderr.",
        "print(\"debug\")",
    ),
    e(
        "__rust_compile",
        "Builtin",
        "__rust_compile(b64 string, line int)",
        "The lowering target of a `rust { … }` block, not something to write by hand. It compiles the base64-encoded Rust body through the host toolchain and registers its `extern \"C\"` exports, which then become callable as ordinary bare-name Go calls.",
        "rust { pub extern \"C\" fn addrs(a: i64, b: i64) -> i64 { a + b } }",
    ),    // ── Package fmt (compiler.rs, the `pkg == "fmt"` arm) ──
    e(
        "fmt.Println",
        "Package fmt",
        "fmt.Println(a …any)",
        "Prints its operands to stdout separated by a single space, followed by a newline. A value whose type has an `Error() string` or `String() string` method prints through that method, via the `$stringify` helper the linker synthesizes when such a type exists.",
        "fmt.Println(\"hello\", 42)",
    ),
    e(
        "fmt.Print",
        "Package fmt",
        "fmt.Print(a …any)",
        "Prints its operands to stdout with no trailing newline. A space is inserted between two operands only when neither of them is a string, matching Go.",
        "fmt.Print(\"x = \", 1)",
    ),
    e(
        "fmt.Printf",
        "Package fmt",
        "fmt.Printf(format string, a …any)",
        "Formats its operands and prints the result to stdout. See the Format Verbs chapter for the verbs, flags, width and precision go-rs implements.",
        "fmt.Printf(\"%d and %s\\n\", 42, \"hi\")",
    ),
    e(
        "fmt.Sprint",
        "Package fmt",
        "fmt.Sprint(a …any) string",
        "The `Print` rendering returned as a string instead of written.",
        "s := fmt.Sprint(\"a\", 1, 2)   // \"a1 2\"",
    ),
    e(
        "fmt.Sprintln",
        "Package fmt",
        "fmt.Sprintln(a …any) string",
        "The `Println` rendering returned as a string, trailing newline included.",
        "s := fmt.Sprintln(\"x\", \"y\")   // \"x y\\n\"",
    ),
    e(
        "fmt.Sprintf",
        "Package fmt",
        "fmt.Sprintf(format string, a …any) string",
        "The `Printf` rendering returned as a string. This is also the workhorse behind `fmt.Errorf`.",
        "s := fmt.Sprintf(\"%d-%s\", 42, \"go\")",
    ),
    e(
        "fmt.Errorf",
        "Package fmt",
        "fmt.Errorf(format string, a …any) error",
        "Builds a real error value. When a program calls it, the linker synthesizes an `$errorString` struct with an `Error() string` method, and the call becomes `&$errorString{s: fmt.Sprintf(format, a…)}`. `%w` is not modelled — it formats like `%v`, and there is no `errors.Is` or `Unwrap` to recover the wrapped error with.",
        "return fmt.Errorf(\"bad input %d\", n)",
    ),
    // ── Format verb (host.rs `sprintf`) ──
    e(
        "%v",
        "Format Verb",
        "%v",
        "The default format. An integer prints in decimal, a whole float without a fractional part (`3`), a slice as `[e0 e1 …]`, a map as `map[k:v …]` with the pairs sorted, a struct as `{f0 f1 …}` with no field names, a function as `<func>`, and nil as `<nil>`.",
        "fmt.Printf(\"%v\", []int{1, 2})   // [1 2]",
    ),
    e(
        "%s",
        "Format Verb",
        "%s",
        "The string form of the operand. In go-rs this is identical to `%v` — every value renders through the same `go_str`, so `%s` on a slice or struct produces the `%v` rendering rather than Go's per-element string conversion.",
        "fmt.Printf(\"%s\", \"hi\")",
    ),
    e(
        "%d",
        "Format Verb",
        "%d",
        "Decimal integer. A float operand is truncated to an integer first. With the `+` flag, a non-negative value gets an explicit sign.",
        "fmt.Printf(\"%d\", 42)",
    ),
    e(
        "%f",
        "Format Verb",
        "%f   |   %F",
        "Fixed-point float with six decimals by default; a precision sets the count. `%F` is a synonym. With `+`, a non-negative value gets an explicit sign.",
        "fmt.Printf(\"%.2f\", 3.14159)   // 3.14",
    ),
    e(
        "%t",
        "Format Verb",
        "%t",
        "Boolean, rendered `true` or `false` through the same conversion as `%v`. A non-boolean operand is not rejected — it renders as `%v` would.",
        "fmt.Printf(\"%t\", ok)",
    ),
    e(
        "%q",
        "Format Verb",
        "%q",
        "The operand wrapped in double quotes. Unlike Go the content is not escaped, so an embedded quote or backslash is emitted verbatim.",
        "fmt.Printf(\"%q\", \"hi\")   // \"hi\"",
    ),
    e(
        "%x",
        "Format Verb",
        "%x",
        "Lowercase hexadecimal of the operand's integer value. A string operand is converted to an integer first rather than hex-encoded byte by byte as in Go.",
        "fmt.Printf(\"%x\", 255)   // ff",
    ),
    e(
        "%X",
        "Format Verb",
        "%X",
        "Uppercase hexadecimal of the operand's integer value.",
        "fmt.Printf(\"%X\", 255)   // FF",
    ),
    e(
        "%o",
        "Format Verb",
        "%o",
        "Octal of the operand's integer value.",
        "fmt.Printf(\"%o\", 8)   // 10",
    ),
    e(
        "%b",
        "Format Verb",
        "%b",
        "Binary of the operand's integer value. Go also accepts `%b` for floats (a power-of-two exponent form); go-rs always renders the integer.",
        "fmt.Printf(\"%b\", 5)   // 101",
    ),
    e(
        "%c",
        "Format Verb",
        "%c",
        "The character whose code point is the operand's integer value. An invalid code point renders as the empty string.",
        "fmt.Printf(\"%c\", 65)   // A",
    ),
    e(
        "%%",
        "Format Verb",
        "%%",
        "A literal percent sign; consumes no operand.",
        "fmt.Printf(\"100%%\\n\")",
    ),
    e(
        "width",
        "Format Verb",
        "%8v",
        "A minimum field width. The rendered value is padded with spaces and right-justified unless the `-` flag is given. Width is counted in characters, not bytes.",
        "fmt.Printf(\"%8.2f|\", 3.14159)   // \"    3.14|\"",
    ),
    e(
        "precision",
        "Format Verb",
        "%.3v",
        "For `%f` and `%F`, the number of decimals. For every other verb, the maximum number of characters kept from the rendered value — so `%.3v` on a string truncates it.",
        "fmt.Printf(\"%.3v\", \"abcdef\")   // abc",
    ),
    e(
        "- flag",
        "Format Verb",
        "%-8v",
        "Left-justify the value within the field width, padding on the right.",
        "fmt.Printf(\"%-4d|\", 9)   // \"9   |\"",
    ),
    e(
        "0 flag",
        "Format Verb",
        "%08d",
        "Zero-fill to the field width, inserted after any sign. Applies only to the numeric verbs `%d %f %F %x %X %o %b`; other verbs fall back to space padding.",
        "fmt.Printf(\"%04d\", 42)   // 0042",
    ),
    e(
        "+ flag",
        "Format Verb",
        "%+d",
        "Always emit a sign for `%d`, `%f` and `%F`, including for non-negative values.",
        "fmt.Printf(\"%+d\", 7)   // +7",
    ),
    e(
        "# and space flags",
        "Format Verb",
        "%#v   |   % d",
        "Both are parsed and then ignored: there is no Go-syntax (`%#v`) rendering and no leading space for the sign of a positive number.",
        "fmt.Printf(\"%#v\", xs)   // same as %v",
    ),
    e(
        "unsupported verbs",
        "Format Verb",
        "%e %g %T %p %U",
        "Not implemented. Any verb go-rs does not recognise falls through to the `%v` renderer, so `%T` prints the value rather than its type and `%e` prints the plain float. No error is reported.",
        "fmt.Printf(\"%T\", 1)   // 1, not int",
    ),
    // ── Package strings (host::stdlib, ids 830-849) ──
    e(
        "strings.ToUpper",
        "Package strings",
        "strings.ToUpper(s string) string",
        "The string with every character mapped to upper case, using full Unicode case mapping.",
        "strings.ToUpper(\"héllo\")   // HÉLLO",
    ),
    e(
        "strings.ToLower",
        "Package strings",
        "strings.ToLower(s string) string",
        "The string with every character mapped to lower case, using full Unicode case mapping.",
        "strings.ToLower(\"HÉLLO\")   // héllo",
    ),
    e(
        "strings.Contains",
        "Package strings",
        "strings.Contains(s, substr string) bool",
        "Whether `substr` occurs in `s`. An empty substring is contained in every string.",
        "strings.Contains(\"seafood\", \"foo\")   // true",
    ),
    e(
        "strings.HasPrefix",
        "Package strings",
        "strings.HasPrefix(s, prefix string) bool",
        "Whether `s` begins with `prefix`.",
        "strings.HasPrefix(\"go-rs\", \"go\")   // true",
    ),
    e(
        "strings.HasSuffix",
        "Package strings",
        "strings.HasSuffix(s, suffix string) bool",
        "Whether `s` ends with `suffix`.",
        "strings.HasSuffix(\"go-rs\", \"rs\")   // true",
    ),
    e(
        "strings.TrimSpace",
        "Package strings",
        "strings.TrimSpace(s string) string",
        "The string with leading and trailing Unicode whitespace removed.",
        "strings.TrimSpace(\"  hi \\n\")   // \"hi\"",
    ),
    e(
        "strings.Split",
        "Package strings",
        "strings.Split(s, sep string) []string",
        "The substrings between each occurrence of `sep`. An empty separator splits into one element per code point — Go splits into one element per UTF-8 byte sequence of each rune, which is the same result for text but differs for invalid UTF-8.",
        "strings.Split(\"a,b\", \",\")   // [a b]",
    ),
    e(
        "strings.Join",
        "Package strings",
        "strings.Join(elems []string, sep string) string",
        "The elements of the slice concatenated with `sep` between them. Every element is rendered with the `%v` conversion first, so a slice of non-strings joins too.",
        "strings.Join([]string{\"a\", \"b\"}, \"-\")   // a-b",
    ),
    e(
        "strings.Repeat",
        "Package strings",
        "strings.Repeat(s string, count int) string",
        "`count` copies of `s`. A negative count clamps to zero and returns the empty string; Go panics instead.",
        "strings.Repeat(\"ab\", 3)   // ababab",
    ),
    e(
        "strings.Index",
        "Package strings",
        "strings.Index(s, substr string) int",
        "The byte index of the first occurrence of `substr`, or -1 when absent.",
        "strings.Index(\"chicken\", \"ken\")   // 4",
    ),
    e(
        "strings.LastIndex",
        "Package strings",
        "strings.LastIndex(s, substr string) int",
        "The byte index of the last occurrence of `substr`, or -1 when absent.",
        "strings.LastIndex(\"go gopher\", \"go\")   // 3",
    ),
    e(
        "strings.Replace",
        "Package strings",
        "strings.Replace(s, old, new string, n int) string",
        "`s` with the first `n` occurrences of `old` replaced by `new`; a negative `n` replaces every occurrence. An empty `old` returns `s` unchanged, where Go would insert `new` between every rune.",
        "strings.Replace(\"aaa\", \"a\", \"b\", 2)   // bba",
    ),
    e(
        "strings.ReplaceAll",
        "Package strings",
        "strings.ReplaceAll(s, old, new string) string",
        "`s` with every occurrence of `old` replaced by `new`. An empty `old` returns `s` unchanged.",
        "strings.ReplaceAll(\"a,b\", \",\", \"-\")   // a-b",
    ),
    e(
        "strings.Fields",
        "Package strings",
        "strings.Fields(s string) []string",
        "The substrings of `s` separated by runs of Unicode whitespace, with empty fields dropped.",
        "len(strings.Fields(\"  a  b \"))   // 2",
    ),
    e(
        "strings.Count",
        "Package strings",
        "strings.Count(s, substr string) int",
        "The number of non-overlapping occurrences of `substr` in `s`. An empty substring returns the number of code points plus one, matching Go.",
        "strings.Count(\"cheese\", \"e\")   // 3",
    ),
    e(
        "strings.TrimPrefix",
        "Package strings",
        "strings.TrimPrefix(s, prefix string) string",
        "`s` without a leading `prefix`, or `s` unchanged when the prefix is absent. Removes at most one occurrence.",
        "strings.TrimPrefix(\"go-rs\", \"go-\")   // rs",
    ),
    e(
        "strings.TrimSuffix",
        "Package strings",
        "strings.TrimSuffix(s, suffix string) string",
        "`s` without a trailing `suffix`, or `s` unchanged when the suffix is absent.",
        "strings.TrimSuffix(\"a.go\", \".go\")   // a",
    ),
    e(
        "strings.Trim",
        "Package strings",
        "strings.Trim(s, cutset string) string",
        "`s` with every leading and trailing character that appears in `cutset` removed.",
        "strings.Trim(\"xxhixx\", \"x\")   // hi",
    ),
    e(
        "strings.Title",
        "Package strings",
        "strings.Title(s string) string",
        "Each space-separated word with its first character upper-cased. It splits on the ASCII space only, where Go's deprecated Title splits on every non-letter.",
        "strings.Title(\"hello wide world\")   // Hello Wide World",
    ),
    e(
        "strings.EqualFold",
        "Package strings",
        "strings.EqualFold(s, t string) bool",
        "Case-insensitive equality — but ASCII-only. Go performs full Unicode case folding, so `EqualFold(\"HÉ\", \"hé\")` is true in Go and false here.",
        "strings.EqualFold(\"Go\", \"GO\")   // true",
    ),
    // ── Package strconv (host::stdlib, ids 850-855) ──
    e(
        "strconv.Itoa",
        "Package strconv",
        "strconv.Itoa(i int) string",
        "The integer rendered in base 10.",
        "strconv.Itoa(42)   // \"42\"",
    ),
    e(
        "strconv.Atoi",
        "Package strconv",
        "strconv.Atoi(s string) int",
        "Parses a base-10 integer, ignoring surrounding whitespace. It returns a single value: a malformed input yields 0 rather than Go's `(int, error)` pair, so the `n, err := strconv.Atoi(s)` idiom does not work here.",
        "n := strconv.Atoi(\"7\")   // 7",
    ),
    e(
        "strconv.ParseInt",
        "Package strconv",
        "strconv.ParseInt(s string, base int) int",
        "Parses an integer in `base` (clamped to a minimum of 2), returning 0 on failure. Go's third `bitSize` argument is accepted and ignored, and only one value is returned rather than `(int64, error)`.",
        "strconv.ParseInt(\"ff\", 16, 64)   // 255",
    ),
    e(
        "strconv.ParseFloat",
        "Package strconv",
        "strconv.ParseFloat(s string) float64",
        "Parses a float, ignoring surrounding whitespace and returning 0 on failure. Go's `bitSize` argument is accepted and ignored, and only one value is returned rather than `(float64, error)`.",
        "strconv.ParseFloat(\"1.25\", 64)   // 1.25",
    ),
    e(
        "strconv.FormatInt",
        "Package strconv",
        "strconv.FormatInt(i int64, base int) string",
        "The integer rendered in base 2, 8, 10 or 16. Go supports every base from 2 to 36; go-rs renders any other base in decimal.",
        "strconv.FormatInt(255, 16)   // \"ff\"",
    ),
    e(
        "strconv.Quote",
        "Package strconv",
        "strconv.Quote(s string) string",
        "The string wrapped in double quotes. The content is not escaped, so unlike Go a control character or embedded quote passes through verbatim.",
        "strconv.Quote(\"hi\")   // \"\\\"hi\\\"\"",
    ),
    // ── Package math (host::stdlib, ids 860-870 and 907-921) ──
    e(
        "math.Abs",
        "Package math",
        "math.Abs(x float64) float64",
        "The absolute value of `x`. Like every `math` function, an integer argument is coerced to a float and the result is a float.",
        "math.Abs(-3)   // 3",
    ),
    e(
        "math.Sqrt",
        "Package math",
        "math.Sqrt(x float64) float64",
        "The square root of `x`; NaN for a negative argument.",
        "math.Sqrt(2)   // 1.4142135623730951",
    ),
    e(
        "math.Pow",
        "Package math",
        "math.Pow(x, y float64) float64",
        "`x` raised to the power `y`.",
        "math.Pow(2, 10)   // 1024",
    ),
    e(
        "math.Floor",
        "Package math",
        "math.Floor(x float64) float64",
        "The greatest integer value not greater than `x`, as a float.",
        "math.Floor(-1.5)   // -2",
    ),
    e(
        "math.Ceil",
        "Package math",
        "math.Ceil(x float64) float64",
        "The least integer value not less than `x`, as a float.",
        "math.Ceil(1.2)   // 2",
    ),
    e(
        "math.Round",
        "Package math",
        "math.Round(x float64) float64",
        "`x` rounded to the nearest integer, halfway cases away from zero.",
        "math.Round(2.5)   // 3",
    ),
    e(
        "math.Trunc",
        "Package math",
        "math.Trunc(x float64) float64",
        "`x` with its fractional part removed, rounding toward zero.",
        "math.Trunc(-1.9)   // -1",
    ),
    e(
        "math.Mod",
        "Package math",
        "math.Mod(x, y float64) float64",
        "The floating-point remainder of `x/y`, taking the sign of `x`.",
        "math.Mod(7, 3)   // 1",
    ),
    e(
        "math.Hypot",
        "Package math",
        "math.Hypot(p, q float64) float64",
        "The square root of p²+q², computed without intermediate overflow.",
        "math.Hypot(3, 4)   // 5",
    ),
    e(
        "math.Max",
        "Package math",
        "math.Max(x, y float64) float64",
        "The larger of two values, always returned as a float — unlike the `max` builtin, which preserves an integer argument's type.",
        "math.Max(3, 7)   // 7",
    ),
    e(
        "math.Min",
        "Package math",
        "math.Min(x, y float64) float64",
        "The smaller of two values, always returned as a float.",
        "math.Min(3, 7)   // 3",
    ),
    e(
        "math.Sin",
        "Package math",
        "math.Sin(x float64) float64",
        "The sine of `x`, in radians.",
        "math.Sin(math.Pi / 2)   // 1",
    ),
    e(
        "math.Cos",
        "Package math",
        "math.Cos(x float64) float64",
        "The cosine of `x`, in radians.",
        "math.Cos(0)   // 1",
    ),
    e(
        "math.Tan",
        "Package math",
        "math.Tan(x float64) float64",
        "The tangent of `x`, in radians.",
        "math.Tan(0)   // 0",
    ),
    e(
        "math.Asin",
        "Package math",
        "math.Asin(x float64) float64",
        "The arcsine of `x`, in radians; NaN outside [-1, 1].",
        "math.Asin(1)   // 1.5707963267948966",
    ),
    e(
        "math.Acos",
        "Package math",
        "math.Acos(x float64) float64",
        "The arccosine of `x`, in radians; NaN outside [-1, 1].",
        "math.Acos(1)   // 0",
    ),
    e(
        "math.Atan",
        "Package math",
        "math.Atan(x float64) float64",
        "The arctangent of `x`, in radians.",
        "math.Atan(1)   // 0.7853981633974483",
    ),
    e(
        "math.Atan2",
        "Package math",
        "math.Atan2(y, x float64) float64",
        "The arctangent of y/x, using the signs of both to select the quadrant.",
        "math.Atan2(1, 1)   // 0.7853981633974483",
    ),
    e(
        "math.Sinh",
        "Package math",
        "math.Sinh(x float64) float64",
        "The hyperbolic sine of `x`.",
        "math.Sinh(0)   // 0",
    ),
    e(
        "math.Cosh",
        "Package math",
        "math.Cosh(x float64) float64",
        "The hyperbolic cosine of `x`.",
        "math.Cosh(0)   // 1",
    ),
    e(
        "math.Tanh",
        "Package math",
        "math.Tanh(x float64) float64",
        "The hyperbolic tangent of `x`.",
        "math.Tanh(0)   // 0",
    ),
    e(
        "math.Exp",
        "Package math",
        "math.Exp(x float64) float64",
        "e raised to the power `x`.",
        "math.Exp(1)   // 2.718281828459045",
    ),
    e(
        "math.Log",
        "Package math",
        "math.Log(x float64) float64",
        "The natural logarithm of `x`.",
        "math.Log(math.E)   // 1",
    ),
    e(
        "math.Log2",
        "Package math",
        "math.Log2(x float64) float64",
        "The base-2 logarithm of `x`.",
        "math.Log2(8)   // 3",
    ),
    e(
        "math.Log10",
        "Package math",
        "math.Log10(x float64) float64",
        "The base-10 logarithm of `x`.",
        "math.Log10(1000)   // 3",
    ),
    e(
        "math.Cbrt",
        "Package math",
        "math.Cbrt(x float64) float64",
        "The cube root of `x`, defined for negative arguments.",
        "math.Cbrt(27)   // 3",
    ),
    e(
        "math.Pi",
        "Package math",
        "math.Pi",
        "The circle constant π, resolved by the compiler to a float constant at the reference site rather than through a call.",
        "area := math.Pi * r * r",
    ),
    e(
        "math.E",
        "Package math",
        "math.E",
        "The base of the natural logarithm, e.",
        "math.Log(math.E)   // 1",
    ),
    e(
        "math.Sqrt2",
        "Package math",
        "math.Sqrt2",
        "The square root of 2, as a float constant.",
        "d := side * math.Sqrt2",
    ),
    e(
        "math.MaxInt64",
        "Package math",
        "math.MaxInt64",
        "The largest int64, 9223372036854775807.",
        "if n == math.MaxInt64 { … }",
    ),
    e(
        "math.MinInt64",
        "Package math",
        "math.MinInt64",
        "The smallest int64, -9223372036854775808.",
        "lo := math.MinInt64",
    ),
    e(
        "math.MaxInt",
        "Package math",
        "math.MaxInt",
        "The largest `int`. go-rs stores every integer in an i64, so this is the same value as `math.MaxInt64` on every target.",
        "best := math.MaxInt",
    ),
    e(
        "math.MinInt",
        "Package math",
        "math.MinInt",
        "The smallest `int`, equal to `math.MinInt64` on every target.",
        "best := math.MinInt",
    ),
    // ── Package sort (host::stdlib ids 875-877, plus the `$sortSlice` lowering) ──
    e(
        "sort.Ints",
        "Package sort",
        "sort.Ints(x []int)",
        "Sorts a slice of integers ascending, in place. Sorting a sub-slice view sorts through the shared backing, so the parent slice sees the reordering.",
        "sort.Ints(xs)",
    ),
    e(
        "sort.Strings",
        "Package sort",
        "sort.Strings(x []string)",
        "Sorts a slice of strings ascending by byte order, in place.",
        "sort.Strings(names)",
    ),
    e(
        "sort.Float64s",
        "Package sort",
        "sort.Float64s(x []float64)",
        "Sorts a slice of floats ascending, in place. Incomparable pairs (NaN) are treated as equal rather than ordered first as in Go.",
        "sort.Float64s(vals)",
    ),
    e(
        "sort.Slice",
        "Package sort",
        "sort.Slice(x any, less func(i, j int) bool)",
        "Sorts `x` using the comparator. A host builtin cannot call a VM closure, so the compiler lowers this to `$sortSlice` — an in-language insertion sort the linker synthesizes into every program, used or not. It is O(n²) and stable.",
        "sort.Slice(xs, func(i, j int) bool { return xs[i] < xs[j] })",
    ),
    e(
        "sort.SliceStable",
        "Package sort",
        "sort.SliceStable(x any, less func(i, j int) bool)",
        "The same `$sortSlice` insertion sort as `sort.Slice`. Because that sort is already stable, the two functions are indistinguishable here.",
        "sort.SliceStable(xs, less)",
    ),
    // ── Package os (host::stdlib id 880) ──
    e(
        "os.Getenv",
        "Package os",
        "os.Getenv(key string) string",
        "The value of the named environment variable, or the empty string when it is unset. It is the only `os` function wired — `os.Exit`, `os.Args`, `os.Open` and the rest are reported as unsupported calls at compile time.",
        "home := os.Getenv(\"HOME\")",
    ),
    // ── Package errors (real Go source, vendored in goroot/errors.go) ──
    e(
        "errors.New",
        "Package errors",
        "errors.New(text string) error",
        "Returns an error whose `Error()` is `text`. This is real Go source: `errors` is not a native builtin package, so the linker parses `goroot/errors.go`, qualifies its names, and merges it into the program.",
        "err := errors.New(\"not found\")",
    ),
    e(
        "errors.ErrUnsupported",
        "Package errors",
        "errors.ErrUnsupported error",
        "The package-level sentinel error value for an unsupported operation. It is initialized by a package-level `var`, which the linker runs before `main`.",
        "return errors.ErrUnsupported",
    ),
    e(
        "Error",
        "Package errors",
        "func (e *errorString) Error() string",
        "The method that makes an `errors.New` value satisfy `error`. Printing such a value with `fmt` goes through it, and calling it explicitly works too.",
        "fmt.Println(err.Error())",
    ),
    // ── Package cmp (real Go source, vendored in goroot/cmp.go) ──
    e(
        "cmp.Ordered",
        "Package cmp",
        "type cmp.Ordered interface",
        "The constraint listing every ordered builtin type. Generic constraints are erased, so it documents intent rather than restricting anything.",
        "func Max[T cmp.Ordered](a, b T) T",
    ),
    e(
        "cmp.Less",
        "Package cmp",
        "cmp.Less(x, y T) bool",
        "Whether `x` orders before `y`, with NaN ordering before every non-NaN.",
        "cmp.Less(1, 2)   // true",
    ),
    e(
        "cmp.Compare",
        "Package cmp",
        "cmp.Compare(x, y T) int",
        "-1, 0 or +1 as `x` orders before, equal to, or after `y`. NaN compares equal to NaN and before everything else.",
        "cmp.Compare(3, 2)   // 1",
    ),
    e(
        "cmp.Or",
        "Package cmp",
        "cmp.Or(vals …T) T",
        "Go returns the first argument that is not the zero value. Here it returns the first argument unconditionally: its `var zero T` erases to nil, and comparing any value to nil reports them unequal, so the first iteration always wins.",
        "cmp.Or(0, 5)   // 0 here, 5 in Go",
    ),
    // ── Package unicode/utf16 (real Go source, vendored in goroot/utf16.go) ──
    e(
        "utf16.IsSurrogate",
        "Package unicode/utf16",
        "utf16.IsSurrogate(r rune) bool",
        "Whether the code point can appear as one half of a surrogate pair.",
        "utf16.IsSurrogate(0xd800)   // true",
    ),
    e(
        "utf16.DecodeRune",
        "Package unicode/utf16",
        "utf16.DecodeRune(r1, r2 rune) rune",
        "The code point a surrogate pair encodes, or U+FFFD when the pair is not valid.",
        "utf16.DecodeRune(0xd83d, 0xde00)",
    ),
    e(
        "utf16.EncodeRune",
        "Package unicode/utf16",
        "utf16.EncodeRune(r rune) (r1, r2 rune)",
        "The surrogate pair for a code point outside the basic multilingual plane; two U+FFFD values when the code point needs no pair.",
        "hi, lo := utf16.EncodeRune(0x1f600)",
    ),
    e(
        "utf16.RuneLen",
        "Package unicode/utf16",
        "utf16.RuneLen(r rune) int",
        "The number of 16-bit words needed to encode the code point: 1, 2, or -1 when it cannot be encoded.",
        "utf16.RuneLen(65)   // 1",
    ),
    e(
        "utf16.Encode",
        "Package unicode/utf16",
        "utf16.Encode(s []rune) []uint16",
        "The UTF-16 encoding of the code points, expanding each one outside the basic multilingual plane into a surrogate pair.",
        "utf16.Encode([]rune(\"hi\"))   // [104 105]",
    ),
    e(
        "utf16.AppendRune",
        "Package unicode/utf16",
        "utf16.AppendRune(a []uint16, r rune) []uint16",
        "The UTF-16 encoding of `r` appended to `a`, returning the extended slice.",
        "a = utf16.AppendRune(a, 'x')",
    ),
    e(
        "utf16.Decode",
        "Package unicode/utf16",
        "utf16.Decode(s []uint16) []rune",
        "The code points a UTF-16 sequence encodes, joining surrogate pairs and replacing unpaired halves with U+FFFD.",
        "string(utf16.Decode([]uint16{104, 105}))   // hi",
    ),
    // ── Operator (lexer.rs token set → ast.rs BinOp / UnOp / AssignOp) ──
    e(
        "+",
        "Operator",
        "a + b",
        "Numeric addition, or string concatenation when either operand is a string. The string case is dispatched at run time through the host's numeric hook, so a mixed `int + string` concatenates rather than failing. Adding nil is the identity, which is how a generic accumulator starting from a type parameter's zero value works.",
        "s := \"go\" + \"-rs\"",
    ),
    e(
        "-",
        "Operator",
        "a - b",
        "Numeric subtraction. Applied to a string it is a reported type error, not a coercion — matching Go's rejection of `\"a\" - 1`.",
        "d := b - a",
    ),
    e(
        "*",
        "Operator",
        "a * b",
        "Numeric multiplication. Rejected on strings.",
        "area := w * h",
    ),
    e(
        "/",
        "Operator",
        "a / b",
        "Division. When the compiler statically types both operands as integers it appends `Op::TruncInt` so the result truncates toward zero; otherwise it is float division. Division by a non-constant zero routes through a checking builtin that raises a Go-style runtime panic.",
        "7 / 2     // 3\n7.0 / 2   // 3.5",
    ),
    e(
        "%",
        "Operator",
        "a % b",
        "Integer remainder, taking the sign of the dividend. A non-constant zero divisor raises the same runtime panic as `/`.",
        "7 % 3   // 1",
    ),
    e(
        "==",
        "Operator",
        "a == b",
        "Equality. The compiler picks a numeric, string, or generic comparison op from the operands' static types. Structs and arrays compare field by field rather than by handle identity, so two separately built structs with equal fields are equal.",
        "if s == \"go\" { … }",
    ),
    e(
        "!=",
        "Operator",
        "a != b",
        "Inequality, with the same type-driven dispatch as `==`. `err != nil` works because an unset result is fusevm's `Undef`.",
        "if err != nil { return err }",
    ),
    e(
        "<",
        "Operator",
        "a < b",
        "Less than. Numeric for numbers, lexicographic by bytes for strings.",
        "\"a\" < \"b\"   // true",
    ),
    e(
        "<=",
        "Operator",
        "a <= b",
        "Less than or equal, with the same numeric/lexicographic split.",
        "if i <= n { … }",
    ),
    e(
        ">",
        "Operator",
        "a > b",
        "Greater than, with the same numeric/lexicographic split.",
        "if score > best { best = score }",
    ),
    e(
        ">=",
        "Operator",
        "a >= b",
        "Greater than or equal, with the same numeric/lexicographic split.",
        "for i >= 0 { i-- }",
    ),
    e(
        "&&",
        "Operator",
        "a && b",
        "Logical AND, short-circuiting: the right operand is only evaluated when the left is true. Lowered as a keep-jump, so no temporary is needed.",
        "if ok && n > 0 { … }",
    ),
    e(
        "||",
        "Operator",
        "a || b",
        "Logical OR, short-circuiting: the right operand is only evaluated when the left is false.",
        "if done || failed { break }",
    ),
    e(
        "!",
        "Operator",
        "!a",
        "Logical negation.",
        "if !ok { return }",
    ),
    e(
        "&",
        "Operator",
        "a & b",
        "Bitwise AND on the operands' integer values.",
        "6 & 3   // 2",
    ),
    e(
        "|",
        "Operator",
        "a | b",
        "Bitwise OR. The same token also appears in a generic constraint union (`~int | ~float64`), where it is erased rather than evaluated.",
        "6 | 3   // 7",
    ),
    e(
        "^",
        "Operator",
        "a ^ b",
        "Bitwise XOR.",
        "6 ^ 3   // 5",
    ),
    e(
        "&^",
        "Operator",
        "a &^ b",
        "Bit clear (AND NOT): the bits of `a` that are not set in `b`.",
        "6 &^ 3   // 4",
    ),
    e(
        "<<",
        "Operator",
        "a << b",
        "Left shift by `b` bits.",
        "1 << 4   // 16",
    ),
    e(
        ">>",
        "Operator",
        "a >> b",
        "Arithmetic right shift by `b` bits.",
        "32 >> 2   // 8",
    ),
    e(
        "^x",
        "Operator",
        "^a",
        "Unary bitwise complement — Go spells this `^`, where C spells it `~`.",
        "^5   // -6",
    ),
    e(
        "-x",
        "Operator",
        "-a",
        "Arithmetic negation. Applied to a string it is a reported type error.",
        "-n",
    ),
    e(
        "&x",
        "Operator",
        "&a",
        "Address-of. go-rs composite values are already heap handles, so this copies nothing and hands back the same handle — `&T{…}` and `T{…}` differ only in that the former skips the struct copy at assignment.",
        "p := &Point{x: 1}",
    ),
    e(
        "*p",
        "Operator",
        "*p",
        "Pointer dereference, which is the identity on a go-rs handle. It exists so pointer-using Go source compiles unchanged.",
        "v := *p",
    ),
    e(
        "<-ch",
        "Operator",
        "<-ch",
        "Channel receive, as an expression. Lowers to `Op::ChanRecv`, which may block the goroutine until a value or a close arrives.",
        "v := <-ch",
    ),
    e(
        "ch <-",
        "Operator",
        "ch <- v",
        "Channel send, which is a statement rather than an expression. Lowers to `Op::ChanSend` and may block when the buffer is full.",
        "ch <- 42",
    ),
    e(
        "++",
        "Operator",
        "x++",
        "Increment by one. A statement in Go and here — it cannot appear inside an expression. The target may be any lvalue: a name, an index, or a field.",
        "for i := 0; i < n; i++ { … }",
    ),
    e(
        "--",
        "Operator",
        "x--",
        "Decrement by one, with the same lvalue rules as `++`.",
        "n--",
    ),
    e(
        "=",
        "Operator",
        "x = v",
        "Assignment to an existing lvalue: a name, an index (`x[i]`), or a field (`x.f`). Assigning a struct copies it.",
        "x = 3",
    ),
    e(
        ":=",
        "Operator",
        "x, y := a, b",
        "Short variable declaration. Declares and initializes one or more names, and is also how a multi-value call is destructured.",
        "v, ok := m[k]",
    ),
    e(
        "+=",
        "Operator",
        "x += v",
        "Compound add-assign. On strings it appends, since it reuses `+`.",
        "total += n",
    ),
    e(
        "-=",
        "Operator",
        "x -= v",
        "Compound subtract-assign.",
        "budget -= cost",
    ),
    e(
        "*=",
        "Operator",
        "x *= v",
        "Compound multiply-assign.",
        "acc *= 2",
    ),
    e(
        "/=",
        "Operator",
        "x /= v",
        "Compound divide-assign, truncating when both sides are statically integers.",
        "n /= 2",
    ),
    e(
        "%=",
        "Operator",
        "x %= v",
        "Compound remainder-assign.",
        "h %= 1000",
    ),
    e(
        "&=",
        "Operator",
        "x &= v",
        "Compound bitwise-AND-assign.",
        "flags &= mask",
    ),
    e(
        "|=",
        "Operator",
        "x |= v",
        "Compound bitwise-OR-assign.",
        "flags |= 1",
    ),
    e(
        "^=",
        "Operator",
        "x ^= v",
        "Compound bitwise-XOR-assign.",
        "h ^= b",
    ),
    e(
        "<<=",
        "Operator",
        "x <<= v",
        "Compound left-shift-assign.",
        "acc <<= 8",
    ),
    e(
        ">>=",
        "Operator",
        "x >>= v",
        "Compound right-shift-assign.",
        "acc >>= 1",
    ),
    e(
        "&^=",
        "Operator",
        "x &^= v",
        "Compound bit-clear-assign.",
        "flags &^= 2",
    ),
    e(
        "...",
        "Operator",
        "f(xs...)   |   func f(args ...T)",
        "Variadic marker. In a parameter list the last parameter binds a slice of the trailing arguments; at a call site it spreads a slice into that parameter. `append(s, xs...)` is the one form with dedicated lowering.",
        "sum(xs...)",
    ),
    e(
        ".",
        "Operator",
        "x.f",
        "Selector. Resolves in order to a package function, a struct field, or a method call. Package selectors are decided at compile time, so `strings.ToUpper` becomes a direct builtin call with no lookup.",
        "p.x",
    ),
    e(
        "[]",
        "Operator",
        "x[i]",
        "Index. On a slice it is bounds-checked and a violation raises `index out of range`; on a map it returns the value or the zero value 0 when the key is absent; on a string it yields the byte at that offset.",
        "xs[2]",
    ),
    e(
        "[lo:hi]",
        "Operator",
        "s[lo:hi]   |   s[lo:]   |   s[:hi]   |   s[:]",
        "Slice expression. On a slice it builds a view sharing the parent's backing array, so element writes are visible both ways; on a string it takes a byte-indexed substring. Both bounds are clamped to the length instead of panicking.",
        "xs[1:3]",
    ),
    e(
        ".(T)",
        "Operator",
        "x.(T)",
        "Type assertion. Succeeds when the value's runtime type tag matches `T`, and raises a recoverable `interface conversion` panic when it does not. An interface target such as `any` always matches. The comma-ok form returns the value and a bool instead of panicking.",
        "s, ok := v.(string)",
    ),
    // ── Statement (ast.rs `Stmt`, plus the top-level declaration forms) ──
    e(
        "package clause",
        "Statement",
        "package NAME",
        "The first statement of every file. go-rs runs `func main` of `package main`; a source package linked by import uses its own name to qualify its top-level identifiers.",
        "package main",
    ),
    e(
        "import declaration",
        "Statement",
        "import \"PATH\"   |   import ( \"PATH\"; … )",
        "Both the single and grouped forms are parsed. Native packages are wired to host builtins; anything else is loaded from Go source and merged, dependencies first, so a package's own globals are initialized before `main` runs.",
        "import (\n\t\"fmt\"\n\t\"strings\"\n)",
    ),
    e(
        "function declaration",
        "Statement",
        "func NAME(params) [results] { … }",
        "Declares a top-level function. Results may be a single type, a parenthesised list, or a named list; naming any result makes them zero-initialized locals a bare `return` yields.",
        "func div(a, b int) (q int, err error) { … }",
    ),
    e(
        "method declaration",
        "Statement",
        "func (r T) NAME(params) [results] { … }",
        "Declares a method. The receiver becomes the first parameter. A pointer receiver (`*T`) is accepted and behaves identically to a value receiver on the handle, so a pointer method mutates the caller's struct while a value method sees a copy.",
        "func (p *Counter) Bump() { p.n++ }",
    ),
    e(
        "type declaration",
        "Statement",
        "type NAME struct { … }   |   type NAME interface { … }",
        "A top-level struct or interface declaration. Field lists may group names (`x, y int`), and a struct may hold slice, map, channel, pointer, function or anonymous-struct fields.",
        "type Point struct { x, y int }",
    ),
    e(
        "var declaration",
        "Statement",
        "var NAME [T] [= expr]",
        "Declares a variable at package level or inside a function. At package level the initializer runs before `main`, in load order.",
        "var limit int = 100",
    ),
    e(
        "const declaration",
        "Statement",
        "const NAME [T] = expr   |   const ( … )",
        "Declares one or more constants. In a grouped block, a spec with no expression repeats the previous spec's expression with `iota` re-substituted at its own index.",
        "const (\n\tKB = 1 << (10 * (iota + 1))\n\tMB\n)",
    ),
    e(
        "short declaration",
        "Statement",
        "x, y := a, b",
        "Declares and initializes names from the right-hand values, or destructures a single call with several results.",
        "v, ok := m[k]",
    ),
    e(
        "assignment",
        "Statement",
        "target op= value",
        "Assigns to one lvalue with `=` or any of the eleven compound operators. The target may be a name, an index, or a field.",
        "counts[k] += 1",
    ),
    e(
        "parallel assignment",
        "Statement",
        "t0, t1, … = v0, v1, …",
        "Assigns to several existing lvalues. Every right-hand side is evaluated before any assignment happens, so `a, b = b, a` swaps without a temporary.",
        "a, b = b, a",
    ),
    e(
        "increment / decrement",
        "Statement",
        "x++   |   x--",
        "Adds or subtracts one from an lvalue. A statement, never an expression.",
        "count++",
    ),
    e(
        "expression statement",
        "Statement",
        "expr",
        "Evaluates an expression for its effect and discards its value — most often a call. The compiler emits the matching `Pop`.",
        "fmt.Println(\"hi\")",
    ),
    e(
        "return statement",
        "Statement",
        "return [expr[, expr …]]",
        "Leaves the function. Several results are packed into a slice the caller destructures. A bare `return` in a function with named results yields their current values; in `main` it ends the program.",
        "return q, nil",
    ),
    e(
        "if statement",
        "Statement",
        "if [init;] cond { … } [else …]",
        "Conditional branch. The optional init statement runs first and its bindings are visible in the condition and both branches.",
        "if v, err := f(); err != nil { return err }",
    ),
    e(
        "for statement",
        "Statement",
        "for [init;] [cond;] [post] { … }",
        "The three-clause loop and its degenerate forms: `for cond { … }` is the while loop and bare `for { … }` is the infinite loop.",
        "for i := 0; i < n; i++ { … }",
    ),
    e(
        "for-range statement",
        "Statement",
        "for [k[, v]] := range iter { … }",
        "Iterates a slice, array, map or string. The key set is materialized once as a slice before the loop starts, so mutating the collection inside the body does not change the iteration. Omit the value, or write `_`, to skip a binding.",
        "for i, v := range xs { … }",
    ),
    e(
        "go statement",
        "Statement",
        "go f(args)",
        "Spawns a goroutine on fusevm's cooperative scheduler. The function and its arguments are evaluated at the statement, then the call is queued.",
        "go worker(id, out)",
    ),
    e(
        "defer statement",
        "Statement",
        "defer f(args)",
        "Pushes a call onto the current function's defer list, drained LIFO at return and during a panic unwind. Arguments are evaluated now.",
        "defer f.Close()",
    ),
    e(
        "send statement",
        "Statement",
        "ch <- v",
        "Sends a value on a channel. Blocks while the buffer is full, or until a receiver is ready on an unbuffered channel.",
        "results <- id * 2",
    ),
    e(
        "select statement",
        "Statement",
        "select { case …: …; default: … }",
        "Waits on several channel operations. Each case is a receive (optionally binding the value) or a send; `default` makes the whole statement non-blocking.",
        "select {\ncase v := <-a:\n\tuse(v)\ncase b <- 1:\ndefault:\n}",
    ),
    e(
        "switch statement",
        "Statement",
        "switch [init;] [tag] { case …: …; default: … }",
        "Tagged or expression multi-way branch. A case may list several expressions; the first match runs and control leaves the switch unless the case ends in `fallthrough`.",
        "switch n {\ncase 1, 2:\n\tsmall()\ndefault:\n\tbig()\n}",
    ),
    e(
        "type switch",
        "Statement",
        "switch [init;] [v :=] x.(type) { case T: … }",
        "Dispatches on the runtime type tag of `x`: `int`, `float64`, `string`, `bool`, a declared struct's name, `[]` for any slice, `map`, `func`, or `nil`. The optional binding names the value inside each case body.",
        "switch v := x.(type) {\ncase int:\n\tfmt.Println(\"int\", v)\ncase string:\n\tfmt.Println(\"str\", v)\n}",
    ),
    e(
        "fallthrough statement",
        "Statement",
        "fallthrough",
        "Runs the next case's body without testing its expression. Legal only as the last statement of a case.",
        "case 2:\n\tp()\n\tfallthrough\ncase 3:\n\tq()",
    ),
    e(
        "break statement",
        "Statement",
        "break",
        "Leaves the innermost loop or switch. Labelled break is not parsed.",
        "if err != nil { break }",
    ),
    e(
        "continue statement",
        "Statement",
        "continue",
        "Starts the next iteration of the innermost loop, skipping any enclosing switch.",
        "if line == \"\" { continue }",
    ),
    e(
        "block statement",
        "Statement",
        "{ … }",
        "A bare block. It groups statements; go-rs does not open a new variable scope for it, so a name declared inside stays visible after it.",
        "{\n\ttmp := compute()\n\tuse(tmp)\n}",
    ),
    e(
        "rust block",
        "Statement",
        "rust { … }",
        "A go-rs extension, not Go. The block's Rust body is extracted before lexing, base64-encoded into a `__rust_compile` call, compiled by the host toolchain, and its `extern \"C\"` exports become callable as ordinary bare-name Go calls in the rest of the program.",
        "rust { pub extern \"C\" fn addrs(a: i64, b: i64) -> i64 { a + b } }\nprintln(addrs(2, 3))",
    ),
    // ── Expression (ast.rs `Expr`) ──
    e(
        "integer literal",
        "Expression",
        "42   0x1F   0o17   0b1010   1_000",
        "Decimal, hexadecimal, octal and binary literals, with `_` digit separators. A value above the i64 maximum is reinterpreted as the i64 with the same bit pattern rather than rejected.",
        "mask := 0b1010_1010",
    ),
    e(
        "float literal",
        "Expression",
        "3.14   1e10   1.5e-3",
        "A float literal keeps both its f64 value and, when it fits, an exact decimal mantissa and scale. Constant expressions fold with that exact form, which is why `0.1 + 0.2` prints `0.3` rather than `0.30000000000000004`.",
        "d := 1.5e-3",
    ),
    e(
        "string literal",
        "Expression",
        "\"text\"   |   `raw`",
        "Interpreted and raw string literals. The interpreted form handles `\\n \\t \\r \\0 \\\\ \\\" \\'`, plus `\\xHH`, `\\uHHHH`, `\\UHHHHHHHH` and octal `\\ooo`.",
        "s := \"tab\\there\"",
    ),
    e(
        "rune literal",
        "Expression",
        "'A'",
        "A single-quoted code point, which is an integer value. The same escape sequences as a string literal apply.",
        "var r rune = '\\u00e9'",
    ),
    e(
        "call",
        "Expression",
        "f(args)",
        "Calls a top-level function, a builtin, a package function, a method, a closure held in a variable, or an FFI export. A closure whose target is only known at run time dispatches through `Op::CallDynamic` on its stored subroutine index.",
        "n := add(1, 2)",
    ),
    e(
        "spread call",
        "Expression",
        "f(xs...)",
        "Expands a slice into a variadic parameter. The compiler materializes each element into a temporary so the callee sees ordinary positional arguments.",
        "total := sum(xs...)",
    ),
    e(
        "slice literal",
        "Expression",
        "[]T{e0, e1, …}",
        "Builds a slice from the element values. Nested composite literals need their element type written out — `[][]int{[]int{1}}`, not Go's elided `[][]int{{1}}`.",
        "xs := []int{1, 2, 3}",
    ),
    e(
        "map literal",
        "Expression",
        "map[K]V{k: v, …}",
        "Builds a map from key/value pairs, with a later duplicate key overwriting an earlier one. As with slice literals, a composite value needs its type written out.",
        "m := map[string]int{\"a\": 1, \"b\": 2}",
    ),
    e(
        "struct literal",
        "Expression",
        "T{v0, v1}   |   T{f0: v0, …}",
        "Builds a struct, positionally or by field name. Omitted fields take their declared type's zero value.",
        "p := Point{x: 1, y: 2}",
    ),
    e(
        "function literal",
        "Expression",
        "func(params) [results] { … }",
        "A closure, compiled to a `$lambda_N` subroutine. Free variables of the body are captured in capture order — by value normally, and through a shared heap cell when the closure assigns to them, which is what makes a counter closure work.",
        "inc := func(n int) int { return n + 1 }",
    ),
    e(
        "immediate call",
        "Expression",
        "func(params) { … }(args)",
        "An immediately-invoked function literal. Compiled to a subroutine and called in place — the usual shape of a `defer`red recover block.",
        "defer func() { recover() }()",
    ),
    e(
        "index expression",
        "Expression",
        "x[i]",
        "A slice element, a map value, or a string byte. A missing map key yields 0, so use the comma-ok form when the zero value is ambiguous.",
        "v := m[\"a\"]",
    ),
    e(
        "comma-ok index",
        "Expression",
        "v, ok := m[k]",
        "Map lookup reporting presence. `ok` is false and `v` is 0 when the key is absent, which distinguishes a missing key from one stored with a zero value.",
        "if v, ok := m[k]; ok { use(v) }",
    ),
    e(
        "slice expression",
        "Expression",
        "s[lo:hi]",
        "A view over a slice's backing array, or a byte-indexed substring of a string. Either bound may be omitted. Out-of-range bounds are clamped rather than raising a panic.",
        "head := xs[:3]",
    ),
    e(
        "selector",
        "Expression",
        "x.f",
        "A package member, a struct field, or a bound method. Field reads and writes go through the struct's field list by name.",
        "p.x = 3",
    ),
    e(
        "type assertion",
        "Expression",
        "x.(T)",
        "Narrows a dynamically typed value to `T`, panicking on a mismatch. The panic is recoverable.",
        "s := v.(string)",
    ),
    e(
        "comma-ok assertion",
        "Expression",
        "v, ok := x.(T)",
        "The non-panicking type assertion: `ok` reports whether the runtime type tag matched, and `v` is the zero value when it did not.",
        "if n, ok := v.(int); ok { use(n) }",
    ),
    e(
        "receive expression",
        "Expression",
        "<-ch",
        "Receives from a channel, blocking until a value or a close arrives.",
        "v := <-results",
    ),
    e(
        "composite address",
        "Expression",
        "&T{…}",
        "A handle to a fresh composite value. Because go-rs values are already handles, this differs from the plain literal only in skipping the struct copy at assignment — which is what gives a pointer receiver its shared-mutation behaviour.",
        "p := &Counter{n: 0}",
    ),
    // ── Divergence from Go (each verified by running the current build) ──
    e(
        "no garbage collector",
        "Divergence from Go",
        "(runtime model)",
        "Composite values live in a host arena that only ever grows: allocation pushes onto a vector and nothing is ever freed or reused. A loop that builds a struct, slice or map per iteration retains every one of them for the life of the process. Programs that allocate in an unbounded loop will exhaust memory where gc-compiled Go would not.",
        "for i := 0; i < n; i++ { p := &Point{} ; _ = p }   // never reclaimed",
    ),
    e(
        "range over a channel",
        "Divergence from Go",
        "for v := range ch",
        "Yields zero iterations. The range lowering asks the host for the iterable's key set, and a channel is a scheduler id rather than a heap object, so the key set comes back empty — whether or not the channel holds values or has been closed. Drain a channel with an explicit counted loop or `select` instead.",
        "for i := 0; i < n; i++ { v := <-ch; use(v) }",
    ),
    e(
        "range over an integer",
        "Divergence from Go",
        "for i := range n",
        "Go 1.22's integer range yields zero iterations here, for the same reason: an integer has no key set. Write the three-clause form.",
        "for i := 0; i < n; i++ { … }",
    ),
    e(
        "%T on a defined type",
        "Divergence from Go",
        "fmt.Printf(\"%T\", d)",
        "A defined type over a non-struct base (`type Weekday int`) is erased by the parser, so `%T` names the base: `int`, not `main.Weekday`. A defined struct type is named correctly, including inside a composite — `[2]main.pt`, `map[string]main.pt`. A fixed-size array carries its written type, so `%T` and `%#v` report `[3]int` rather than `[]int`.",
        "fmt.Printf(\"%T\", a) // [3]int",
    ),
    e(
        "call argument limit",
        "Divergence from Go",
        "fmt.Println(a1, a2, /* … 256 … */)",
        "A call passes at most 255 arguments: fusevm holds a call's argument count in a `u8`. Over that is a compile error rather than a silently short call. Go has no such limit. A composite literal is not bounded this way — it is built in chunks, so `[]T{…}`, `map[K]V{…}` and a struct literal work at any size.",
        "fmt.Println(xs) // pass the slice",
    ),
    e(
        "func init",
        "Divergence from Go",
        "func init()",
        "Parsed as an ordinary function and never called. Package-level `var` initializers do run before `main`, so move initialization there.",
        "var registry = buildRegistry()",
    ),
    e(
        "append aliasing",
        "Divergence from Go",
        "append(s, v)",
        "The non-spread form extends the backing array in place and returns the same handle, so the append is visible through every alias of `s`. Go copies to a new array once capacity is exceeded, which detaches the result. Code that relies on that detachment behaves differently here.",
        "b := append(a, 1)   // a and b stay the same slice",
    ),
    e(
        "interface{} in a type position",
        "Divergence from Go",
        "var x interface{}",
        "Not parsed. `interface{ … }` is accepted in a `type` declaration and as a generic constraint, but not where a variable, parameter or field type is expected. Write `any`.",
        "var x any",
    ),
    e(
        "embedded fields",
        "Divergence from Go",
        "type D struct { Base }",
        "Anonymous (embedded) struct fields are not parsed — every field needs a name. There is no field or method promotion; write the field explicitly and forward the methods.",
        "type D struct { base Base }",
    ),
    e(
        "local type declarations",
        "Divergence from Go",
        "func f() { type T … }",
        "Only top-level `type` declarations are parsed. A `type` inside a function body is a syntax error; move it to file scope.",
        "type T struct{ a int }\n\nfunc f() { … }",
    ),
    e(
        "elided composite element types",
        "Divergence from Go",
        "[][]int{{1, 2}}",
        "Go lets a nested composite literal omit its element type. go-rs requires it in both slice and map literals.",
        "[][]int{[]int{1, 2}}",
    ),
    e(
        "generics are erased",
        "Divergence from Go",
        "func F[T any](…)",
        "Type parameters, constraint interfaces and type arguments are parsed and discarded — the dynamic value model runs one compiled body for every instantiation. The visible consequence is that `var zero T` is nil rather than the concrete type's zero value, which is what breaks `cmp.Or`.",
        "func Sum[T Number](xs []T) T",
    ),
    e(
        "labels and goto",
        "Divergence from Go",
        "L: for { break L }",
        "Statement labels, labelled `break`/`continue`, and `goto` are not implemented. Restructure with a flag or an extra function.",
        "done := false\nfor !done { … }",
    ),
    e(
        "unsupported package calls",
        "Divergence from Go",
        "pkg.Func(…)",
        "Calling a name a native package does not wire is a compile-time error, not a link error: `go-rs: unsupported call \\`fmt.Fprintln\\``. The wired sets are exactly the ones this reference lists — 6 in fmt, 20 in strings, 6 in strconv, 26 functions and 7 constants in math, 5 in sort, and 1 in os.",
        "fmt.Sprintf(…)   // wired\nfmt.Fprintf(…)   // not",
    ),
    e(
        "map iteration order",
        "Divergence from Go",
        "for k := range m",
        "Go deliberately randomizes map iteration order. go-rs stores a map as an association list and ranges it in insertion order, so iteration is deterministic. Do not rely on that determinism in code meant to also build with the Go toolchain.",
        "for k, v := range m { … }   // insertion order",
    ),
];

/// The corpus, exposed for offline doc generation.
pub fn corpus() -> &'static [Entry] {
    CORPUS
}

/// Render `go doc [name]` from the corpus. With a name, print that one entry's
/// category, signature, description, and example (case-sensitive exact match
/// first, then a case-insensitive fallback). Without a name, print the full
/// index grouped by category. Returns the text to print, or an error string if
/// `name` is unknown.
pub fn doc(name: Option<&str>) -> Result<String, String> {
    let Some(name) = name else {
        // Full index, grouped by category in first-seen order.
        let mut out = String::from("go-rs reference — documented surfaces\n");
        let mut cats: Vec<&str> = Vec::new();
        for entry in CORPUS {
            if !cats.contains(&entry.chapter) {
                cats.push(entry.chapter);
            }
        }
        for cat in cats {
            out.push_str(&format!("\n{cat}\n"));
            for entry in CORPUS {
                if entry.chapter == cat {
                    out.push_str(&format!("  {:<22} {}\n", entry.name, entry.signature));
                }
            }
        }
        return Ok(out);
    };

    let entry = CORPUS.iter().find(|entry| entry.name == name).or_else(|| {
        CORPUS
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
    });
    match entry {
        Some(entry) => Ok(format!(
            "{n}  ({cat})\n\n    {sig}\n\n{doc}\n\nexample:\n    {example}\n",
            n = entry.name,
            cat = entry.chapter,
            sig = entry.signature,
            doc = entry.doc,
            example = entry.example.replace('\n', "\n    "),
        )),
        None => Err(format!(
            "go-rs: no documentation for `{name}` (try `go doc` for the index)"
        )),
    }
}

/// Open document text keyed by URI, kept current from the sync notifications so
/// hover can look up the identifier under the cursor.
type Docs = HashMap<String, String>;

/// Entry point for `go --lsp`.
pub fn run() -> Result<(), String> {
    spawn_orphan_guard();
    let (conn, io_threads) = Connection::stdio();
    let (init_id, _params) = conn
        .initialize_start()
        .map_err(|e| format!("lsp initialize: {e}"))?;
    let init_result = serde_json::json!({
        "capabilities": server_capabilities(),
        "serverInfo": { "name": "go-rs", "version": env!("CARGO_PKG_VERSION") },
    });
    conn.sender
        .send(Response::new_ok(init_id, init_result).into())
        .map_err(|e| format!("lsp send: {e}"))?;

    let mut docs: Docs = HashMap::new();
    for msg in &conn.receiver {
        match msg {
            Message::Request(req) => {
                if conn
                    .handle_shutdown(&req)
                    .map_err(|e| format!("lsp shutdown: {e}"))?
                {
                    break;
                }
                dispatch_request(&conn, &docs, req);
            }
            Message::Notification(not) => dispatch_notification(&conn, &mut docs, not),
            Message::Response(_) => {}
        }
    }
    drop(conn);
    io_threads.join().map_err(|_| "lsp io join".to_string())?;
    Ok(())
}

fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::FULL),
                ..Default::default()
            },
        )),
        completion_provider: Some(CompletionOptions {
            resolve_provider: Some(false),
            ..Default::default()
        }),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        ..Default::default()
    }
}

fn handle<P, R>(conn: &Connection, req: Request, f: impl FnOnce(P) -> R)
where
    P: serde::de::DeserializeOwned,
    R: serde::Serialize,
{
    let method = req.method.clone();
    let id = req.id.clone();
    match req.extract::<P>(&method) {
        Ok((id, params)) => {
            let value = serde_json::to_value(f(params)).unwrap_or(serde_json::Value::Null);
            let _ = conn.sender.send(Response::new_ok(id, value).into());
        }
        Err(ExtractError::JsonError { error, .. }) => {
            let _ = conn.sender.send(
                Response::new_err(id, ErrorCode::InvalidParams as i32, error.to_string()).into(),
            );
        }
        Err(ExtractError::MethodMismatch(_)) => unreachable!("method matched before extract"),
    }
}

fn dispatch_request(conn: &Connection, docs: &Docs, req: Request) {
    match req.method.as_str() {
        Completion::METHOD => handle(conn, req, |_p: CompletionParams| completions()),
        HoverRequest::METHOD => handle(conn, req, |p: HoverParams| hover(docs, &p)),
        _ => {
            let _ = conn.sender.send(
                Response::new_err(req.id, ErrorCode::MethodNotFound as i32, "unhandled".into())
                    .into(),
            );
        }
    }
}

fn dispatch_notification(conn: &Connection, docs: &mut Docs, not: lsp_server::Notification) {
    match not.method.as_str() {
        DidOpenTextDocument::METHOD => {
            if let Ok(p) = serde_json::from_value::<DidOpenTextDocumentParams>(not.params) {
                let uri = p.text_document.uri;
                docs.insert(uri.as_str().to_string(), p.text_document.text.clone());
                publish_diagnostics(conn, &uri, &p.text_document.text);
            }
        }
        DidChangeTextDocument::METHOD => {
            if let Ok(p) = serde_json::from_value::<DidChangeTextDocumentParams>(not.params) {
                if let Some(change) = p.content_changes.into_iter().last() {
                    let uri = p.text_document.uri;
                    docs.insert(uri.as_str().to_string(), change.text.clone());
                    publish_diagnostics(conn, &uri, &change.text);
                }
            }
        }
        DidCloseTextDocument::METHOD => {
            if let Ok(p) = serde_json::from_value::<DidCloseTextDocumentParams>(not.params) {
                let uri = p.text_document.uri;
                docs.remove(uri.as_str());
                publish_diagnostics(conn, &uri, "");
            }
        }
        _ => {}
    }
}

/// The completion-item kind for a chapter: keywords and predeclared identifiers
/// are keywords, type-shaped chapters are classes, everything else is a callable.
fn completion_kind(chapter: &str) -> CompletionItemKind {
    match chapter {
        "Keyword" | "Predeclared Identifier" | "Statement" | "Operator" => {
            CompletionItemKind::KEYWORD
        }
        "Type" => CompletionItemKind::CLASS,
        _ => CompletionItemKind::METHOD,
    }
}

fn completions() -> CompletionResponse {
    let items = CORPUS
        .iter()
        .map(|entry| CompletionItem {
            label: entry.name.to_string(),
            kind: Some(completion_kind(entry.chapter)),
            detail: Some(entry.signature.to_string()),
            documentation: Some(lsp_types::Documentation::String(entry.doc.to_string())),
            ..Default::default()
        })
        .collect();
    CompletionResponse::Array(items)
}

/// Hover: look up the identifier under the cursor in the corpus and render its
/// chapter, signature, doc, and example. Falls back to a short banner when the
/// cursor is not on a known name.
fn hover(docs: &Docs, params: &HoverParams) -> Hover {
    let pos = params.text_document_position_params.position;
    let uri = params
        .text_document_position_params
        .text_document
        .uri
        .as_str();
    let word = docs
        .get(uri)
        .and_then(|text| word_at(text, pos))
        .unwrap_or_default();

    let matches: Vec<&Entry> = CORPUS.iter().filter(|entry| entry.name == word).collect();

    let body = if matches.is_empty() {
        "**go-rs** — Go on the fusevm bytecode VM + Cranelift JIT.".to_string()
    } else {
        let mut out = String::new();
        for entry in matches {
            out.push_str(&format!(
                "**`{name}`** — _{chapter}_\n\n```go\n{sig}\n```\n\n{doc}\n\n```go\n{example}\n```\n\n",
                name = entry.name,
                chapter = entry.chapter,
                sig = entry.signature,
                doc = entry.doc,
                example = entry.example,
            ));
        }
        out.trim_end().to_string()
    };

    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: body,
        }),
        range: None,
    }
}

/// Extract the identifier (`[A-Za-z0-9_]+`) spanning the given position, if any.
fn word_at(text: &str, pos: Position) -> Option<String> {
    let line = text.lines().nth(pos.line as usize)?;
    let chars: Vec<char> = line.chars().collect();
    let col = (pos.character as usize).min(chars.len());
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';

    let mut start = col;
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    let mut end = col;
    while end < chars.len() && is_word(chars[end]) {
        end += 1;
    }
    if start == end {
        return None;
    }
    Some(chars[start..end].iter().collect())
}

fn publish_diagnostics(conn: &Connection, uri: &Uri, text: &str) {
    let params = PublishDiagnosticsParams {
        uri: uri.clone(),
        diagnostics: compute_diagnostics(text),
        version: None,
    };
    let not = lsp_server::Notification::new(PublishDiagnostics::METHOD.to_string(), params);
    let _ = conn.sender.send(not.into());
}

/// Parse the whole document with the runtime's own parser; a syntax error maps
/// to a single diagnostic on the line named in its `on line N` / `(line N)`
/// suffix.
fn compute_diagnostics(text: &str) -> Vec<Diagnostic> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    match crate::parse(text) {
        Ok(_) => Vec::new(),
        Err(e) => {
            let line = parse_error_line(&e).saturating_sub(1);
            vec![Diagnostic {
                range: Range {
                    start: Position { line, character: 0 },
                    end: Position {
                        line,
                        character: 200,
                    },
                },
                severity: Some(DiagnosticSeverity::ERROR),
                message: e,
                ..Default::default()
            }]
        }
    }
}

/// Extract the (1-based) line number from a go-rs parser error, which embeds it
/// as `… on line N` or `… (line N)`. Defaults to line 1 when no marker is present.
fn parse_error_line(e: &str) -> u32 {
    let after = e
        .rsplit_once("on line ")
        .map(|(_, rest)| rest)
        .or_else(|| e.rsplit_once("(line ").map(|(_, rest)| rest));
    after
        .and_then(|rest| {
            rest.split(|c: char| !c.is_ascii_digit())
                .find(|s| !s.is_empty())
        })
        .and_then(|n| n.parse().ok())
        .unwrap_or(1)
}

/// Exit if reparented to pid 1 (the editor died) so we never leak.
fn spawn_orphan_guard() {
    std::thread::spawn(|| {
        #[cfg(target_os = "linux")]
        // SAFETY: prctl(PR_SET_PDEATHSIG, ...) only registers a signal disposition.
        unsafe {
            libc::prctl(
                libc::PR_SET_PDEATHSIG,
                libc::SIGKILL as libc::c_ulong,
                0,
                0,
                0,
            );
        }
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            // SAFETY: getppid takes no arguments and never fails.
            if unsafe { libc::getppid() } == 1 {
                std::process::exit(0);
            }
        }
    });
}
