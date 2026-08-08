# Known parity gaps

Behavioural differences between go-rs and the reference `go` toolchain that are
known and not yet closed. Each entry is a reproducer, what Go prints, what go-rs
prints, and what it would take to close.

Found by the differential harnesses:

- `bash parity-scripts/run.sh` — byte-diffs every `parity-scripts/**/*.go`
  against `go run`. Currently 28/28.
- `cargo build --bin parity-fuzz && ./target/debug/parity-fuzz --count 20000`
  — generated deterministic-output programs, byte-diffed the same way.

A gap listed here is deliberately **not** represented by a corpus file, because
the corpus is a green byte-parity gate. Close the gap and add the corpus file in
the same change.

## Fixed-width integer arithmetic does not wrap to its declared width

```go
var i8 int8 = 127
i8++            // go: -128     go-rs: 128
var u8 uint8 = 0
u8--            // go: 255      go-rs: -1
```

Conversions already truncate correctly (`int8(300)` → 44, `int32(5e9)` →
705032704), and `int`/`int64` arithmetic wraps at 64 bits. Only the narrower
declared widths are unmodelled: every integer is a 64-bit `Value::Int`, and
nothing records that a variable is 8/16/32-bit or unsigned. Closing it needs a
width tag on the value (or a typed-slot model in the compiler) so `++`, `+` and
the shift operators can mask to the declared width.

## `float32` prints with `float64` precision

```go
var f := float32(1.0 / 3.0)
fmt.Println(f)  // go: 0.33333334     go-rs: 0.3333333333333333
```

The conversion itself is right — `float32(x)` does round to `f32` — but `fmt`
formats every float with the shortest representation that round-trips as an
`f64`. Go picks the shortest that round-trips at the value's own width. Needs
the same width tag as the integer case; `format_float` then selects the 32-bit
shortest digits.

## A nil slice or nil map prints as `<nil>`

```go
var s []int
var m map[string]int
fmt.Println(s, m)   // go: [] map[]     go-rs: <nil> <nil>
fmt.Println(m["x"]) // go: 0            go-rs: go-rs: invalid index of nil
```

`len`, `cap` and `append` on a nil slice are already correct. The zero value is
`Value::Undef`, which carries no element type, so the printer cannot tell a nil
slice from a nil map from a nil interface. A distinguished heap object per kind
would print correctly, but `s == nil` compiles to fusevm's generic equality
against `Undef` — making it true for that object needs an equality hook go-rs
does not currently install.

## A pointer to a struct prints without `&`

```go
p := point{1, "a"}
fmt.Println(&p)  // go: &{1 a}     go-rs: {1 a}
```

go-rs models `&x` as the same heap handle as `x` (structs are already
reference-shaped on the host heap), so nothing distinguishes the pointer from
the value at print time. Needs a pointer wrapper object.

## `append` capacity misses Go's malloc size-class rounding

```go
var s []int
s = append(s, 1, 2, 3, 4, 5)
fmt.Println(cap(s))   // go: 6     go-rs: 5
```

`runtime.nextslicecap` is ported faithfully (see `next_slice_cap` in
`src/host.rs`), so the repeated-single-append doubling sequence
`1 2 4 4 8 8 8 8 16 …` matches exactly. Go then rounds the new backing array's
**byte** size up to a malloc size class — 5 ints is 40 bytes, which rounds to
the 48-byte class, giving cap 6. go-rs has no static element type at run time,
so it cannot compute the byte size. Sniffing it from the element values would
give the wrong answer for `[]byte` (1 byte) and for struct elements, so it is
left unrounded rather than confidently wrong.

## The three-index slice `a[lo:hi:max]` ignores `max`

```go
base := make([]int, 5, 5)
fmt.Println(cap(base[1:2:3]))  // go: 2     go-rs: 4
```

`max` is parsed and discarded. A `HostObj::SliceView`'s capacity is derived as
`backing.len() - offset`, so there is nowhere to record a capacity smaller than
the backing allows. Needs a `cap` field on `SliceView`.

## `errors.Is` / `errors.As` / `errors.Unwrap` and `%w` are missing

```go
e2 := fmt.Errorf("wrap: %w", e)
errors.Is(e2, e)   // go-rs: undefined: errors.Is
```

`errors.New`, the `error` interface, `Error()` dispatch and `fmt.Errorf` with
`%v` all work. The vendored `goroot/errors.go` stops at `New`. Porting the rest
faithfully needs two things go-rs does not have: `%w` recording a wrapped error
rather than formatting it, and assertions against anonymous interface types
(`err.(interface{ Unwrap() error })`), which is how the real `Is`/`As` walk the
chain. `As` additionally needs `reflectlite`.

## `strconv` conversions always report a nil error

```go
n, err := strconv.Atoi("xx")
// go:    0 strconv.Atoi: parsing "xx": invalid syntax
// go-rs: 0 <nil>
```

`Atoi`, `ParseInt` and `ParseFloat` return a bare value, and the compiler pads
the extra assignment names with nil. The tuple-destructuring path used by
`GMAP_GET2` would carry a real `(value, error)` pair, but go-rs also accepts
`strconv.Atoi(s)` in single-value expression position (`tests/eval.rs:361`
asserts `strconv.Atoi("100")+1`), which real Go rejects. Changing the return
shape has to update that test in the same change.

## `sync` is not vendored

```go
var wg sync.WaitGroup   // go-rs: expected identifier, found `Star` on line 123
```

`sync` is not in `goroot/`, so `pkg` falls back to the local toolchain's
`$GOROOT/src/sync`, whose real source uses constructs go-rs cannot parse. The
goroutine and channel primitives themselves work; only the `sync` types are
unreachable. Needs either a go-rs-compatible vendored `sync` or host builtins
for `WaitGroup`/`Mutex`.

## Unsupported stdlib calls

`fmt.Fprintln` / `fmt.Fprintf` (writer-directed output) and
`strconv.FormatFloat` are rejected at compile time.

## Constant-overflow is not diagnosed

```go
fmt.Println(int8(300))
// go:    compile error — constant 300 overflows int8
// go-rs: 44
```

go-rs has no constant-range checking pass, so an out-of-range constant
conversion silently truncates instead of failing the build.
