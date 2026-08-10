//! Differential parity fuzzer: reference `go run` vs the built go-rs `go run`.
//!
//! Generates grammar-driven, deterministic-output Go programs, runs each through
//! both the reference `go` toolchain and the freshly-built go-rs binary (in
//! parallel across CPU cores), and reports every case where stdout OR exit
//! status diverges. Each case is a per-index seed so any divergence replays
//! exactly: `parity-fuzz --seed <N> --once`.
//!
//! Modeled on the sibling frontends' harnesses (rubylang `parity_fuzz`, the
//! zshrs parity fuzzer): splitmix64 PRNG, seed→program generator, byte differ,
//! parallel workers, a divergence report file.
//!
//! Determinism invariant: the generator only emits constructs whose output is
//! deterministic and identical across a correct implementation — no goroutine
//! scheduling order, integer `/` and `%` with a guaranteed-nonzero divisor, and
//! maps printed via `fmt` (which sorts keys on both sides). Pure random bytes
//! would only produce mutual syntax errors that agree and teach nothing.
//!
//! Floats are emitted both with an explicit `%.4f` and — since the shortest-`g`
//! thresholds are implemented — with `%v`/`%g`/`%e`, which is where exponent
//! notation (`1e+06`) appears.
//!
//! Build: cargo build --bin parity-fuzz
//! Run:   ./target/debug/parity-fuzz --count 20000 --jobs 12

use std::io::Write as _;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

// ── splitmix64 PRNG (no `rand` dependency) ─────────────────────────────────

struct Rng(u64);

impl Rng {
    fn seed(s: u64) -> Rng {
        Rng(s ^ 0x9E37_79B9_7F4A_7C15)
    }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn int(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.below((hi - lo + 1) as u64) as i64)
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len() as u64) as usize]
    }
}

// ── expression generators ──────────────────────────────────────────────────

fn int_expr(rng: &mut Rng, depth: u32) -> String {
    if depth == 0 || rng.below(3) == 0 {
        return rng.int(-9, 20).to_string();
    }
    match rng.below(6) {
        0 => format!(
            "({} + {})",
            int_expr(rng, depth - 1),
            int_expr(rng, depth - 1)
        ),
        1 => format!(
            "({} - {})",
            int_expr(rng, depth - 1),
            int_expr(rng, depth - 1)
        ),
        2 => format!(
            "({} * {})",
            int_expr(rng, depth - 1),
            int_expr(rng, depth - 1)
        ),
        // Guaranteed-nonzero divisor so both sides agree (go panics on /0).
        3 => format!("({} / {})", int_expr(rng, depth - 1), rng.int(1, 9)),
        4 => format!("({} % {})", int_expr(rng, depth - 1), rng.int(1, 9)),
        _ => rng.int(-9, 20).to_string(),
    }
}

/// A *constant* float expression: literal leaves and `+ - * /` only. go-rs
/// constant-folds these with exact rational arithmetic and rounds once, matching
/// Go's arbitrary-precision constant semantics, so both interpreters agree. Uses
/// one-fractional-digit decimals and safe divisors so the exact terms stay in
/// the `f64`-exact range.
fn const_float_expr(rng: &mut Rng, depth: u32) -> String {
    // One fractional digit (denominator 10) keeps the exact rational terms well
    // inside the f64-exact range even after a few operations, so go-rs folds
    // rather than falling back to runtime f64.
    let lit = |rng: &mut Rng| format!("{}.{}", rng.int(1, 12), rng.below(10));
    if depth == 0 || rng.below(3) == 0 {
        return lit(rng);
    }
    match rng.below(4) {
        0 => format!(
            "({} + {})",
            const_float_expr(rng, depth - 1),
            const_float_expr(rng, depth - 1)
        ),
        1 => format!(
            "({} - {})",
            const_float_expr(rng, depth - 1),
            const_float_expr(rng, depth - 1)
        ),
        2 => format!(
            "({} * {})",
            const_float_expr(rng, depth - 1),
            const_float_expr(rng, depth - 1)
        ),
        // Divide by a small non-zero literal.
        _ => format!(
            "({} / {}.{})",
            const_float_expr(rng, depth - 1),
            rng.int(1, 12),
            rng.below(10) + 1
        ),
    }
}

/// A float expression over the given (runtime) variables. Leaves are variables,
/// NOT literals: any variable forces the runtime `f64` path (double-rounded)
/// that go-rs implements, so combining variables keeps both sides on the same
/// footing. Constant (literal) float arithmetic is covered separately by
/// [`const_float_expr`], which go-rs folds exactly.
fn float_expr(rng: &mut Rng, vars: &[String], depth: u32) -> String {
    if depth == 0 || vars.is_empty() || rng.below(3) == 0 {
        return rng.pick(vars).clone();
    }
    match rng.below(4) {
        0 => format!(
            "({} + {})",
            float_expr(rng, vars, depth - 1),
            float_expr(rng, vars, depth - 1)
        ),
        1 => format!(
            "({} - {})",
            float_expr(rng, vars, depth - 1),
            float_expr(rng, vars, depth - 1)
        ),
        2 => format!(
            "({} * {})",
            float_expr(rng, vars, depth - 1),
            float_expr(rng, vars, depth - 1)
        ),
        // Divide by (var + 1.0): variables are declared from non-negative
        // literals, so this is always ≥ 1 and never divides by zero.
        _ => format!(
            "({} / ({} + 1.0))",
            float_expr(rng, vars, depth - 1),
            rng.pick(vars)
        ),
    }
}

/// Declare three runtime float variables from non-negative literals, returning
/// the declaration statements and the variable names.
fn float_vars(rng: &mut Rng, n: u64) -> (String, Vec<String>) {
    let names: Vec<String> = (0..3).map(|k| format!("f{k}_{n}")).collect();
    let mut decl = String::new();
    for name in &names {
        decl.push_str(&format!(
            "\t{name} := {}.{:03}\n",
            rng.int(0, 12),
            rng.below(1000)
        ));
    }
    // Go rejects unused variables; a blank-assign counts as a use, so declaring
    // three vars is safe even if the generated expression references only some.
    for name in &names {
        decl.push_str(&format!("\t_ = {name}\n"));
    }
    (decl, names)
}

fn bool_expr(rng: &mut Rng, depth: u32) -> String {
    if depth == 0 || rng.below(2) == 0 {
        let op = rng.pick(&["<", "<=", ">", ">=", "==", "!="]);
        return format!("({} {} {})", int_expr(rng, 1), op, int_expr(rng, 1));
    }
    match rng.below(3) {
        0 => format!(
            "({} && {})",
            bool_expr(rng, depth - 1),
            bool_expr(rng, depth - 1)
        ),
        1 => format!(
            "({} || {})",
            bool_expr(rng, depth - 1),
            bool_expr(rng, depth - 1)
        ),
        _ => format!("(!{})", bool_expr(rng, depth - 1)),
    }
}

const WORDS: &[&str] = &[
    "go", "rs", "fuse", "vm", "abc", "xyz", "", "hello", "Ox", "zz",
];

fn str_expr(rng: &mut Rng, depth: u32) -> String {
    if depth == 0 || rng.below(2) == 0 {
        return format!("\"{}\"", rng.pick(WORDS));
    }
    format!(
        "({} + {})",
        str_expr(rng, depth - 1),
        str_expr(rng, depth - 1)
    )
}

// ── statement-block generators (each prints deterministic output) ───────────

/// How many statement-block shapes [`block`] can emit. `--only N` pins every
/// block of every program to shape `N`, which is what makes a newly added shape
/// measurable on its own: mixed into the other 31 it would contribute a handful
/// of statements per program and its divergence rate would be unreadable.
const SHAPES: u64 = 39;

/// Emit a random block of statements. `n` is a fresh var-name suffix. `only`
/// pins the shape instead of drawing one.
fn block(rng: &mut Rng, n: u64, uses: &mut Uses, only: Option<u64>) -> String {
    match only.unwrap_or_else(|| rng.below(SHAPES)) {
        // ── unsigned 64-bit integers ──────────────────────────────────────
        // `uint64`/`uint`/`uintptr` share `int64`'s bit pattern, so every
        // operation that reads the sign bit — `/`, `%`, `>>`, the ordered
        // comparisons, the conversion to a float, and printing — is a place a
        // frontend that stores one `i64` can silently produce a negative answer.
        27 => {
            // Every literal here must fit an `int64` as written: `1<<64 - 1`
            // names the same value but only an arbitrary-precision constant
            // evaluator can fold it, which is a separate documented gap (see
            // BUGS.md) and would keep this shape permanently red for a reason
            // that has nothing to do with unsigned *runtime* semantics.
            let big = *rng.pick(&[
                "18446744073709551615",
                "9223372036854775808",
                "12297829382473034410",
                "1 << 63",
                "1 << 62",
                "1 << 61",
            ]);
            let (small, sh) = (rng.int(1, 97), rng.int(0, 5));
            format!(
                "\tvar ua{n} uint64 = {big}\n\
                 \tvar ub{n} uint64 = {small}\n\
                 \tfmt.Println(ua{n}-ub{n}, ua{n}+ub{n}, ua{n}*3, ua{n}/ub{n}, ua{n}%ub{n})\n\
                 \tfmt.Println(ua{n}>>{sh}, ua{n}<<{sh}, ua{n} < ub{n}, ua{n} > ub{n}, ua{n} == ub{n}, ua{n} >= ub{n})\n\
                 \tfmt.Printf(\"%d|%x|%o|%b|%v\\n\", ua{n}, ua{n}, ua{n}, ua{n}, ua{n})\n\
                 \tvar uc{n} uint = {big}\n\
                 \tuc{n} /= {small}\n\
                 \tfmt.Println(uc{n}, uc{n}>>{sh}, uc{n} > {small})\n\
                 \tvar up{n} uintptr = {big}\n\
                 \tfmt.Println(up{n}, up{n}/7, float64(ua{n}))\n"
            )
        }
        // ── narrow fixed-width integers ───────────────────────────────────
        // Wrapping at 8/16/32 bits, arithmetic vs logical `>>`, shifts at or
        // past the width (defined in Go, unlike C), and conversion truncation.
        // Every operand is a *variable*, because Go rejects an out-of-range
        // constant conversion at compile time and go-rs has no such pass.
        28 => {
            // `b` is the divisor of `i8_{n}/j8_{n}` further down, so it must
            // never be 0: Go panics on integer divide-by-zero, which would end
            // the program early and make the case a mutual crash rather than a
            // comparison of printed values.
            let a = rng.int(-128, 127);
            let b = {
                let v = rng.int(-127, 126);
                if v >= 0 {
                    v + 1
                } else {
                    v
                }
            };
            let (c, d) = (rng.int(0, 255), rng.int(0, 255));
            let (e, f) = (rng.int(-32768, 32767), rng.int(0, 65535));
            let (g, sh) = (rng.int(-2147483648, 2147483647), rng.int(0, 40));
            format!(
                "\tvar i8_{n} int8 = {a}\n\tvar j8_{n} int8 = {b}\n\
                 \tvar u8_{n} uint8 = {c}\n\tvar v8_{n} uint8 = {d}\n\
                 \tvar i16_{n} int16 = {e}\n\tvar u16_{n} uint16 = {f}\n\
                 \tvar i32_{n} int32 = {g}\n\
                 \ti8_{n} += j8_{n}\n\tu8_{n} *= v8_{n}\n\ti16_{n} -= int16(i8_{n})\n\
                 \tfmt.Println(i8_{n}, u8_{n}, i16_{n}, u16_{n}, i32_{n})\n\
                 \tfmt.Println(i8_{n}>>{sh}, u8_{n}>>{sh}, i8_{n}<<{sh}, u8_{n}<<{sh}, i32_{n}>>{sh})\n\
                 \tfmt.Println(int8(i16_{n}), uint8(i32_{n}), int16(u16_{n}), int32(i16_{n}), uint16(i32_{n}))\n\
                 \tfmt.Println(int64(i8_{n}), uint32(u16_{n}), i8_{n}/j8_{n}|1, i16_{n}%7)\n\
                 \tcap{n} := func() {{\n\
                 \t\ti8_{n} += j8_{n}\n\tu8_{n} *= v8_{n}\n\
                 \t\tfmt.Println(i8_{n}, u8_{n}, i8_{n}>>{sh}, u8_{n}<<{sh}, i32_{n}>>{sh})\n\
                 \t}}\n\tcap{n}()\n\
                 \tfmt.Println(i8_{n}, u8_{n})\n"
            )
        }
        // ── break / continue, labelled and not ────────────────────────────
        // Six sibling frontends have each hit a different variant of a loop
        // signal escaping the wrong chunk, so every form is exercised nested.
        29 => {
            let (x, y) = (rng.int(2, 5), rng.int(2, 5));
            let (p, q, r, s) = (rng.int(0, 4), rng.int(0, 12), rng.int(0, 4), rng.int(0, 4));
            format!(
                "\tL{n}:\n\
                 \tfor a{n} := 0; a{n} < {x}; a{n}++ {{\n\
                 \t\tfor b{n} := 0; b{n} < {y}; b{n}++ {{\n\
                 \t\t\tif b{n} == {p} {{\n\t\t\t\tcontinue L{n}\n\t\t\t}}\n\
                 \t\t\tif a{n}*{y}+b{n} > {q} {{\n\t\t\t\tbreak L{n}\n\t\t\t}}\n\
                 \t\t\tif b{n} == {r} {{\n\t\t\t\tcontinue\n\t\t\t}}\n\
                 \t\t\tif a{n} == {s} {{\n\t\t\t\tbreak\n\t\t\t}}\n\
                 \t\t\tfmt.Println(\"ab\", a{n}, b{n})\n\
                 \t\t}}\n\t}}\n\
                 \tw{n} := 0\n\tk{n} := 0\n\
                 \tfor {{\n\t\tk{n}++\n\
                 \t\tif k{n} > {x}*{y} {{\n\t\t\tbreak\n\t\t}}\n\
                 \t\tif k{n}%2 == 0 {{\n\t\t\tcontinue\n\t\t}}\n\
                 \t\tw{n} += k{n}\n\t}}\n\
                 \tfor z{n} := 0; z{n} < {x}; z{n}++ {{\n\
                 \t\tswitch z{n} % 3 {{\n\
                 \t\tcase 0:\n\t\t\tfmt.Println(\"zero\")\n\t\t\tfallthrough\n\
                 \t\tcase 1:\n\t\t\tfmt.Println(\"one\")\n\
                 \t\tcase 2:\n\t\t\tfmt.Println(\"two\")\n\t\t\tfallthrough\n\
                 \t\tdefault:\n\t\t\tfmt.Println(\"def\")\n\t\t}}\n\t}}\n\
                 \tfmt.Println(\"w\", w{n})\n"
            )
        }
        // ── defer / panic / recover ───────────────────────────────────────
        // LIFO order, a `defer` that runs on the panic path, a `recover()` made
        // *after* the deferred function has called something else, and a
        // `recover()` from a nested call — which Go defines as ineffective.
        30 => {
            uses.deferred = true;
            let (v, do_panic) = (rng.int(0, 99), rng.below(2) == 0);
            let cond = if do_panic { "true" } else { "false" };
            format!(
                "\tfunc() {{\n\
                 \t\tdefer fmt.Println(\"last{n}\")\n\
                 \t\tdefer func() {{\n\
                 \t\t\tnoise()\n\
                 \t\t\tif rec := recover(); rec != nil {{\n\t\t\t\tfmt.Println(\"rec{n}\", rec)\n\t\t\t}}\n\
                 \t\t}}()\n\
                 \t\tdefer fmt.Println(\"first{n}\")\n\
                 \t\tif {cond} {{\n\t\t\tpanic({v})\n\t\t}}\n\
                 \t\tfmt.Println(\"fell through{n}\")\n\
                 \t}}()\n\
                 \tfmt.Println(doubled({v}))\n\
                 \tfunc() {{\n\
                 \t\tdefer func() {{\n\
                 \t\t\tf := func() any {{ return recover() }}\n\
                 \t\t\tfmt.Println(\"nested{n}\", f())\n\
                 \t\t\tfmt.Println(\"direct{n}\", recover())\n\
                 \t\t}}()\n\
                 \t\tpanic({v} + 1)\n\
                 \t}}()\n\
                 \tfor q{n} := 0; q{n} < 3; q{n}++ {{\n\t\tdefer fmt.Println(\"loopdefer{n}\", q{n})\n\t}}\n"
            )
        }
        // ── channels ──────────────────────────────────────────────────────
        // Deterministic by construction: every channel is filled to capacity
        // and closed before it is read, so no output depends on scheduling.
        // Covers `range ch`, `v, ok := <-ch`, `select` with `default`, and the
        // comma-ok `select` case a closed channel makes ready.
        31 => {
            let cap = rng.int(1, 5);
            let (m, k) = (rng.int(-9, 9), rng.int(0, 40));
            let w = rng.pick(WORDS);
            format!(
                "\tch{n} := make(chan int, {cap})\n\
                 \tfor i{n} := 0; i{n} < {cap}; i{n}++ {{\n\t\tch{n} <- i{n} * {m}\n\t}}\n\
                 \tclose(ch{n})\n\
                 \tsum{n} := 0\n\tcnt{n} := 0\n\
                 \tfor v{n} := range ch{n} {{\n\t\tsum{n} += v{n}\n\t\tcnt{n}++\n\t}}\n\
                 \tfmt.Println(\"range\", sum{n}, cnt{n})\n\
                 \tsc{n} := make(chan string, 1)\n\tsc{n} <- \"{w}\"\n\tclose(sc{n})\n\
                 \ts1{n}, o1{n} := <-sc{n}\n\ts2{n}, o2{n} := <-sc{n}\n\
                 \tfmt.Println(s1{n}, o1{n}, s2{n}, o2{n}, len(s2{n}))\n\
                 \tbc{n} := make(chan bool, 1)\n\tbc{n} <- false\n\tclose(bc{n})\n\
                 \tb1{n}, p1{n} := <-bc{n}\n\tb2{n}, p2{n} := <-bc{n}\n\
                 \tfmt.Println(b1{n}, p1{n}, b2{n}, p2{n})\n\
                 \tdc{n} := make(chan int, 1)\n\
                 \tselect {{\n\tcase x{n} := <-dc{n}:\n\t\tfmt.Println(\"got\", x{n})\n\tdefault:\n\t\tfmt.Println(\"empty\")\n\t}}\n\
                 \tdc{n} <- {k}\n\
                 \tselect {{\n\tcase x{n} := <-dc{n}:\n\t\tfmt.Println(\"got\", x{n})\n\tdefault:\n\t\tfmt.Println(\"empty\")\n\t}}\n\
                 \tec{n} := make(chan int, 1)\n\tclose(ec{n})\n\
                 \tselect {{\n\tcase y{n}, ok{n} := <-ec{n}:\n\t\tfmt.Println(\"closed\", y{n}, ok{n})\n\tdefault:\n\t\tfmt.Println(\"default\")\n\t}}\n\
                 \tgc{n} := make(chan int, {cap})\n\
                 \tgo func() {{\n\t\tfor i{n} := 1; i{n} <= {cap}; i{n}++ {{\n\t\t\tgc{n} <- i{n}\n\t\t}}\n\t\tclose(gc{n})\n\t}}()\n\
                 \tgs{n} := 0\n\tfor v{n} := range gc{n} {{\n\t\tgs{n} += v{n}\n\t}}\n\
                 \tfmt.Println(\"goroutine\", gs{n})\n"
            )
        }
        // Shortest-representation float output (`%v`, `Println`, `%g`). This is
        // the form that switches to `1e+06`-style exponent notation, so it
        // exercises strconv's 'g' thresholds rather than a fixed `%.4f`.
        25 => {
            let (decl, vars) = float_vars(rng, n);
            let f = float_expr(rng, &vars, 2);
            format!("{decl}\tfmt.Println({f})\n\tfmt.Printf(\"%v|%g|%e\\n\", {f}, {f}, {f})\n")
        }
        // Integer division and modulo through a slice element, whose numeric
        // type go-rs only learns at run time.
        26 => {
            let (a, b, c) = (rng.int(-40, 90), rng.int(1, 12), rng.int(-9, 9));
            format!(
                "\tq{n} := []int{{{a}, {c}}}\n\td{n} := []int{{{b}, {b}}}\n\tfmt.Println(q{n}[0]/d{n}[0], q{n}[0]%d{n}[0], q{n}[1]/d{n}[1], q{n}[1]%d{n}[1])\n"
            )
        }
        0 => format!(
            "\tfmt.Printf(\"%d %d\\n\", {}, {})\n",
            int_expr(rng, 3),
            int_expr(rng, 3)
        ),
        1 => {
            let (decl, vars) = float_vars(rng, n);
            format!(
                "{decl}\tfmt.Printf(\"%.4f\\n\", {})\n",
                float_expr(rng, &vars, 2)
            )
        }
        2 => format!(
            "\tfmt.Println({}, {})\n",
            bool_expr(rng, 2),
            bool_expr(rng, 2)
        ),
        3 => format!("\tfmt.Println({})\n", str_expr(rng, 2)),
        // if / else
        4 => format!(
            "\tif {} {{\n\t\tfmt.Println(\"T\")\n\t}} else {{\n\t\tfmt.Println(\"F\")\n\t}}\n",
            bool_expr(rng, 2)
        ),
        // for-accumulate
        5 => {
            let lim = rng.int(0, 12);
            let k = rng.int(-3, 4);
            format!(
                "\ts{n} := 0\n\tfor i{n} := 0; i{n} < {lim}; i{n}++ {{\n\t\ts{n} += i{n} * {k}\n\t}}\n\tfmt.Println(s{n})\n"
            )
        }
        // slice build + sort + print
        6 => {
            uses.sort = true;
            let (a, b, c, d, e) = (
                rng.int(-9, 30),
                rng.int(-9, 30),
                rng.int(-9, 30),
                rng.int(-9, 30),
                rng.int(-9, 30),
            );
            format!(
                "\txs{n} := []int{{{a}, {b}, {c}, {d}, {e}}}\n\tsort.Ints(xs{n})\n\tsum{n} := 0\n\tfor _, v := range xs{n} {{\n\t\tsum{n} += v\n\t}}\n\tfmt.Println(xs{n}, sum{n})\n"
            )
        }
        // map build + print (fmt sorts keys on both sides)
        7 => {
            let (x, y, z) = (rng.int(0, 9), rng.int(0, 9), rng.int(0, 9));
            format!(
                "\tm{n} := map[string]int{{\"a\": {x}, \"b\": {y}}}\n\tm{n}[\"c\"] = {z}\n\tm{n}[\"a\"] += 5\n\tdelete(m{n}, \"b\")\n\tfmt.Println(m{n}, len(m{n}))\n"
            )
        }
        // strings stdlib
        8 => {
            uses.strings = true;
            let s = str_expr(rng, 1);
            let sub = format!("\"{}\"", rng.pick(WORDS));
            format!(
                "\tfmt.Println(strings.ToUpper({s}), strings.Contains({s}, {sub}), strings.Count({s}, {sub}))\n"
            )
        }
        // constant float expression (folded exactly by go-rs, matching Go's
        // arbitrary-precision constant rounding)
        9 => format!("\tfmt.Printf(\"%.8f\\n\", {})\n", const_float_expr(rng, 2)),
        // math stdlib (fixed precision so both format identically)
        10 => {
            uses.math = true;
            let (decl, vars) = float_vars(rng, n);
            let f = float_expr(rng, &vars, 1);
            let g = float_expr(rng, &vars, 1);
            format!(
                "{decl}\tfmt.Printf(\"%.4f %.4f %.0f\\n\", math.Sqrt({f}), math.Abs(-({g})), math.Floor({f}))\n"
            )
        }
        // rune literals as int32 code points: arithmetic, difference, and
        // string(rune) conversion.
        11 => {
            let x = rng.int(0, 25);
            format!("\tfmt.Println('A'+{x}, 'z'-'0', string(rune(97+{x})))\n")
        }
        // fixed-size array literal + range sum.
        12 => {
            let (a, b, c, d) = (
                rng.int(-9, 30),
                rng.int(-9, 30),
                rng.int(-9, 30),
                rng.int(-9, 30),
            );
            format!(
                "\tarr{n} := [4]int{{{a}, {b}, {c}, {d}}}\n\tas{n} := 0\n\tfor _, v := range arr{n} {{\n\t\tas{n} += v\n\t}}\n\tfmt.Println(arr{n}, len(arr{n}), as{n})\n"
            )
        }
        // sparse index-keyed array literal (zero-filled gaps).
        13 => {
            let (x, y, z) = (rng.int(1, 9), rng.int(1, 9), rng.int(1, 9));
            format!(
                "\tsp{n} := [5]int{{0: {x}, 2: {y}, 4: {z}}}\n\tfmt.Println(sp{n}, len(sp{n}))\n"
            )
        }
        // []byte / []rune conversions and string() back.
        14 => {
            let w = rng.pick(WORDS);
            format!(
                "\tbb{n} := []byte(\"{w}\")\n\trr{n} := []rune(\"{w}\")\n\tfmt.Println(bb{n}, len(bb{n}), len(rr{n}), string(bb{n}), string(rr{n}))\n"
            )
        }
        // range over a string yields runes: sum the code points.
        15 => {
            let w = rng.pick(WORDS);
            format!(
                "\tcp{n} := 0\n\tfor _, c := range \"{w}\" {{\n\t\tcp{n} += int(c)\n\t}}\n\tfmt.Println(cp{n})\n"
            )
        }
        // three-index (full) slice expression.
        16 => {
            let (a, b, c, d, e, f) = (
                rng.int(-5, 9),
                rng.int(-5, 9),
                rng.int(-5, 9),
                rng.int(-5, 9),
                rng.int(-5, 9),
                rng.int(-5, 9),
            );
            format!(
                "\txs{n} := []int{{{a}, {b}, {c}, {d}, {e}, {f}}}\n\tp{n} := xs{n}[1:4:6]\n\tfmt.Println(p{n}, len(p{n}))\n"
            )
        }
        // struct value + pointer-receiver method mutation.
        17 => {
            uses.structs = true;
            let (x, y, k) = (rng.int(-9, 20), rng.int(-9, 20), rng.int(-3, 5));
            format!(
                "\tp{n} := pt{{{x}, {y}}}\n\tq{n} := p{n}\n\tq{n}.scale({k})\n\tfmt.Println(p{n}.sum(), q{n}.sum(), q{n}.x, q{n}.y)\n"
            )
        }
        // ── struct value semantics ────────────────────────────────────────
        // Go copies a struct at every assignment, argument bind, return,
        // container store, container read, range binding, channel send and
        // value-receiver call — and does it *transitively*, so a nested struct
        // field is copied too. A frontend that models a struct as a shared
        // handle passes the flat case (`pt` in shape 17) while every nested one
        // writes through to the original, which is silent data corruption
        // rather than a visibly wrong answer. Every value here is printed after
        // a deliberate write through the copy, so any missed copy shows up.
        32 => {
            uses.nested = true;
            let (a, b, d) = (rng.int(-9, 20), rng.int(-9, 20), rng.int(1, 7));
            let (w, x, y) = (rng.int(-5, 9), rng.int(-5, 9), rng.int(-5, 9));
            format!(
                "\tn{n} := node{{leaf{{{a}}}, {b}}}\n\
                 \tc{n} := n{n}\n\tc{n}.k = {w}\n\tc{n}.l.n = {x}\n\
                 \tfmt.Println(\"asg\", n{n}.k, n{n}.l.n, c{n}.k, c{n}.l.n)\n\
                 \tfmt.Println(\"vrecv\", n{n}.valSum(), n{n}.k, n{n}.l.n)\n\
                 \tn{n}.ptrBump({d})\n\tfmt.Println(\"precv\", n{n}.k, n{n}.l.n)\n\
                 \tf{n} := func(v node) node {{ v.k = {y}; v.l.n = {y}; return v }}\n\
                 \tg{n} := f{n}(n{n})\n\tfmt.Println(\"call\", n{n}.k, n{n}.l.n, g{n}.k, g{n}.l.n)\n\
                 \txs{n} := []node{{n{n}, c{n}}}\n\txs{n} = append(xs{n}, g{n})\n\
                 \te{n} := xs{n}[0]\n\te{n}.l.n = {w}\n\
                 \tfmt.Println(\"idx\", xs{n}[0].l.n, e{n}.l.n)\n\
                 \tfor _, v{n} := range xs{n} {{\n\t\tv{n}.k = {x}\n\t\tv{n}.l.n = {x}\n\t}}\n\
                 \tfmt.Println(\"rng\", xs{n}[0].k, xs{n}[1].l.n, xs{n}[2].k)\n\
                 \tys{n} := append([]node{{}}, xs{n}...)\n\tys{n}[0].l.n = {y}\n\
                 \tfmt.Println(\"spread\", xs{n}[0].l.n, ys{n}[0].l.n)\n\
                 \tm{n} := map[string]node{{\"a\": n{n}}}\n\tm{n}[\"b\"] = c{n}\n\
                 \th{n} := m{n}[\"b\"]\n\th{n}.l.n = {a}\n\
                 \tfmt.Println(\"map\", m{n}[\"b\"].l.n, h{n}.l.n)\n\
                 \tk{n} := make(chan node, 1)\n\tk{n} <- n{n}\n\tr{n} := <-k{n}\n\tr{n}.l.n = {b}\n\
                 \tfmt.Println(\"chan\", n{n}.l.n, r{n}.l.n)\n\
                 \tfmt.Println(\"eq\", n{n} == c{n}, c{n} == c{n}, node{{leaf{{{a}}}, {b}}} == node{{leaf{{{a}}}, {b}}})\n\
                 \tfmt.Println(\"val\", n{n}, c{n}, g{n}, xs{n}, m{n}[\"a\"])\n"
            )
        }
        // ── fixed-size array value semantics ──────────────────────────────
        // A Go array is a *value*, like a struct and unlike a slice: it is
        // copied at assignment, argument bind, return, container read and
        // store, channel send, `append`, and `range` — and elementwise, so an
        // array of arrays or of structs separates at every depth while an array
        // of slices keeps sharing. Shapes 12 and 13 already built arrays, but
        // only ever read them back, so a frontend that modeled one as a slice
        // handle scored clean on both. Every value here is printed after a
        // deliberate write through what Go says is an independent copy.
        33 => {
            uses.structs = true;
            uses.arrays = true;
            let (a, b, c) = (rng.int(-9, 20), rng.int(-9, 20), rng.int(-9, 20));
            let (d, w, x, y) = (
                rng.int(1, 7),
                rng.int(-5, 9),
                rng.int(-5, 9),
                rng.int(-5, 9),
            );
            format!(
                "\ta{n} := [3]int{{{a}, {b}, {c}}}\n\tb{n} := a{n}\n\tb{n}[0] = {w}\n\
                 \tfmt.Println(\"asg\", a{n}, b{n})\n\
                 \tc{n} := bumpArr(a{n}, {d})\n\tfmt.Println(\"call\", a{n}, c{n})\n\
                 \tnz{n} := [2][2]int{{{{{a}, {b}}}, {{{c}, {w}}}}}\n\tmz{n} := nz{n}\n\tmz{n}[0][0] = {x}\n\
                 \tfmt.Println(\"nest\", nz{n}, mz{n})\n\
                 \tg{n} := grid{{a: [2]int{{{a}, {b}}}, q: [2]pt{{{{{c}, {w}}}, {{{x}, {y}}}}}}}\n\
                 \th{n} := g{n}\n\th{n}.a[1] = {x}\n\th{n}.q[0].x = {y}\n\
                 \tfmt.Println(\"field\", g{n}.a, h{n}.a, g{n}.q, h{n}.q)\n\
                 \txs{n} := [][2]int{{{{{a}, {b}}}, {{{c}, {w}}}}}\n\
                 \te{n} := xs{n}[0]\n\te{n}[0] = {x}\n\tfmt.Println(\"idx\", xs{n}[0], e{n})\n\
                 \ts{n} := [2]int{{{x}, {y}}}\n\txs{n}[1] = s{n}\n\ts{n}[0] = {d}\n\
                 \tfmt.Println(\"store\", xs{n}[1], s{n})\n\
                 \tys{n} := append(xs{n}, s{n})\n\ts{n}[1] = {d}\n\
                 \tzs{n} := append([][2]int{{}}, xs{n}...)\n\tzs{n}[0][0] = {w}\n\
                 \tfmt.Println(\"append\", ys{n}[2], xs{n}[0], zs{n}[0])\n\
                 \tm{n} := map[string][2]int{{\"a\": s{n}}}\n\tv{n} := m{n}[\"a\"]\n\tv{n}[0] = {y}\n\
                 \tfmt.Println(\"map\", m{n}[\"a\"], v{n})\n\
                 \tkm{n} := map[[2]int]int{{{{{a}, {b}}}: {x}}}\n\
                 \tfmt.Println(\"key\", km{n}[[2]int{{{a}, {b}}}], len(km{n}))\n\
                 \tk{n} := make(chan [3]int, 1)\n\tk{n} <- a{n}\n\trv{n} := <-k{n}\n\trv{n}[1] = {y}\n\
                 \tfmt.Println(\"chan\", a{n}, rv{n})\n\
                 \trs{n} := 0\n\tfor i{n}, ev{n} := range a{n} {{\n\t\tif i{n} == 0 {{\n\t\t\ta{n}[1] = {d}\n\t\t}}\n\t\trs{n} += ev{n}\n\t}}\n\
                 \tfmt.Println(\"rng\", rs{n}, a{n})\n\
                 \tfor _, ev{n} := range xs{n} {{\n\t\tev{n}[0] = {w}\n\t}}\n\
                 \tfmt.Println(\"rngelem\", xs{n})\n\
                 \tsh{n} := [2][]int{{{{{a}}}, {{{b}}}}}\n\tsk{n} := sh{n}\n\tsk{n}[0][0] = {x}\n\tsk{n}[1] = []int{{{y}}}\n\
                 \tfmt.Println(\"share\", sh{n}, sk{n})\n\
                 \tvar zv{n} [2][2]int\n\tvar zg{n} grid\n\
                 \tfmt.Println(\"zero\", zv{n}, zg{n})\n\
                 \tfmt.Println(\"eq\", [2]int{{{a}, {b}}} == [2]int{{{a}, {b}}}, a{n} == b{n}, nz{n} == mz{n})\n"
            )
        }
        // `%T` / `%#v` name a fixed-size array by its length, and the length is
        // part of the type. go-rs holds a `[N]T` and a `[]T` in the same heap
        // object, so the name has to ride on the value itself — through an
        // assignment, an `any` box, a container read and a `fmt` width box —
        // while the slice spellings alongside must still name a slice.
        34 => {
            uses.structs = true;
            let (a, b, c) = (rng.int(-9, 20), rng.int(-9, 20), rng.int(-9, 20));
            let k = rng.int(0, 4);
            let w = rng.pick(WORDS);
            format!(
                "\ta{n} := [3]int{{{a}, {b}, {c}}}\n\ts{n} := []int{{{a}, {b}, {c}}}\n\
                 \tfmt.Printf(\"%T %T\\n\", a{n}, s{n})\n\
                 \tfmt.Printf(\"%#v %#v\\n\", a{n}, s{n})\n\
                 \tfmt.Printf(\"%v %v\\n\", a{n}, s{n})\n\
                 \tvar z{n} [{k}]string\n\tfmt.Printf(\"%T %v\\n\", z{n}, z{n})\n\
                 \tb{n} := a{n}\n\tfmt.Printf(\"%T\\n\", b{n})\n\
                 \tvar i{n} any = a{n}\n\tfmt.Printf(\"%T\\n\", i{n})\n\
                 \tn{n} := [2][3]int{{{{{a}, {b}, {c}}}, {{{c}, {a}, {b}}}}}\n\
                 \tfmt.Printf(\"%T %#v\\n\", n{n}, n{n})\n\
                 \tp{n} := [2]pt{{{{{a}, {b}}}, {{{c}, {a}}}}}\n\
                 \tfmt.Printf(\"%T %#v\\n\", p{n}, p{n})\n\
                 \tq{n} := [2][]int{{{{{a}}}, {{{b}}}}}\n\tfmt.Printf(\"%T\\n\", q{n})\n\
                 \tr{n} := [][2]int{{{{{a}, {b}}}}}\n\tfmt.Printf(\"%T %T\\n\", r{n}, r{n}[0])\n\
                 \tm{n} := map[string][2]int{{\"{w}\": {{{a}, {b}}}}}\n\
                 \tfmt.Printf(\"%T\\n\", m{n}[\"{w}\"])\n\
                 \tvar f{n} [2]float32\n\tvar u{n} [2]uint64\n\
                 \tfmt.Printf(\"%T %T\\n\", f{n}, u{n})\n\
                 \tfmt.Printf(\"%T\\n\", a{n}[:])\n"
            )
        }
        // A composite literal is not bounded by the 255 stack values one
        // fusevm `CallBuiltin` carries, so one past that is built in chunks.
        // The count used to wrap, and the literal silently came out short —
        // hence the checks on an element past the cut, not just the length.
        35 => {
            uses.bulk = true;
            // At least 256, so the literal is over the limit *and* the index
            // just past it below is in range — Go rejects a constant index off
            // the end at compile time, which would only ever be a skip.
            let k = rng.int(256, 320) as usize;
            let pairs = rng.int(120, 140) as usize;
            let base = rng.int(-9, 9);
            let (last, plast) = (k - 1, pairs - 1);
            let elems = (0..k)
                .map(|i| (i as i64 + base).to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let mps = (0..pairs)
                .map(|i| format!("{i}: {}", i as i64 * 2 + base))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "\tbs{n} := []int{{{elems}}}\n\
                 \tfmt.Println(\"biglen\", len(bs{n}), bs{n}[0], bs{n}[254], bs{n}[255], bs{n}[{last}])\n\
                 \tba{n} := [{k}]int{{{elems}}}\n\
                 \tfmt.Println(\"bigarr\", len(ba{n}), ba{n}[255], ba{n}[{last}])\n\
                 \tbc{n} := ba{n}\n\tbc{n}[{last}] = 0\n\
                 \tfmt.Println(\"bigcopy\", ba{n}[{last}], bc{n}[{last}])\n\
                 \tbm{n} := map[int]int{{{mps}}}\n\
                 \tfmt.Println(\"bigmap\", len(bm{n}), bm{n}[0], bm{n}[126], bm{n}[127], bm{n}[{plast}])\n\
                 \tfmt.Println(\"bigsum\", sumAll(bs{n}...), sumAll({elems}))\n"
            )
        }
        // Every `fmt` verb except `%v` and `%T` applies *element-wise* to a
        // composite operand: `%q` of a `[]string` quotes each element, `%d` of a
        // map renders each key and value, `%f` of a `[]float64` formats each
        // number — and the flags, width and precision belong to each element,
        // not to the whole rendering. A verb that renders the composite as one
        // value instead prints something shaped nothing like Go's answer.
        //
        // The one operand that is *not* a list is a `[]byte` under `%s`, `%q`,
        // `%x` or `%X`: it is the text it holds, at every depth (a `[][]byte`
        // prints its rows as strings). Nothing in the values separates a
        // `[]byte` from a `[]int` of small numbers, so both halves are only
        // right at once if the written element type reaches the formatter —
        // which is why the two are always printed side by side here.
        36 => {
            uses.structs = true;
            let (w1, w2) = (rng.pick(WORDS), rng.pick(WORDS));
            // `%c`/`%q`/`%U` on the rune classes whose printability go-rs
            // decides exactly: C0 and C1 controls, the ASCII range, a non-ASCII
            // separator, printable symbols in and above the basic plane, the
            // replacement character, both private-use planes, one past the last
            // code point, and a negative (which `%U` reads as all 64 bits).
            // Deliberately absent: a code point Unicode has not assigned, whose
            // category needs tables go-rs does not carry — see BUGS.md.
            let a = rng.int(-9, 120);
            let b = *rng.pick(&[
                0, 7, 0x1f, 0x20, 0x41, 0x7f, 0x80, 0x85, 0x9f, 0xa0, 0xa9, 0x2764, 0x4e16, 0xfffd,
                0x1f600, 0xe000, 0x10fffd, 1114112,
            ]);
            let f = rng.pick(&["1.5", "2.25", "-0.75", "10", "0.125"]);
            let wd = rng.int(2, 9);
            format!(
                "\tqw{n} := []string{{\"{w1}\", \"{w2}\"}}\n\
                 \tqi{n} := []int{{{a}, {b}}}\n\
                 \tqb{n} := []byte(\"{w1}\")\n\
                 \tqm{n} := map[string]string{{\"{w1}\": \"{w2}\", \"z\": \"{w1}\"}}\n\
                 \tqn{n} := map[string]int{{\"{w2}\": {a}}}\n\
                 \tqa{n} := [2]string{{\"{w1}\", \"{w2}\"}}\n\
                 \tqf{n} := []float64{{{f}, 2.5}}\n\
                 \tqz{n} := [][]string{{{{\"{w1}\"}}, {{\"{w2}\", \"z\"}}}}\n\
                 \tqy{n} := [2][]byte{{[]byte(\"{w1}\"), []byte(\"{w2}\")}}\n\
                 \tvar qs{n} []string\n\tvar qc{n} []byte\n\
                 \tqp{n} := pt{{{a}, {b}}}\n\
                 \tfmt.Printf(\"%q|%q|%q|%q\\n\", qw{n}, qi{n}, qb{n}, qm{n})\n\
                 \tfmt.Printf(\"%q|%q|%q|%q\\n\", qa{n}, qf{n}, qz{n}, qy{n})\n\
                 \tfmt.Printf(\"%q|%q|%q\\n\", qs{n}, qc{n}, qp{n})\n\
                 \tfmt.Printf(\"%d|%d|%d|%d\\n\", qi{n}, qn{n}, qz{n}, qp{n})\n\
                 \tfmt.Printf(\"%s|%s|%s|%s\\n\", qw{n}, qi{n}, qb{n}, qm{n})\n\
                 \tfmt.Printf(\"%x|%X|%x|%x\\n\", qb{n}, qi{n}, qm{n}, qy{n})\n\
                 \tfmt.Printf(\"%f|%e|%g\\n\", qf{n}, qf{n}, qf{n})\n\
                 \tfmt.Printf(\"%o|%b|%c|%U\\n\", qi{n}, qi{n}, qi{n}, qi{n})\n\
                 \tfmt.Printf(\"%t|%t\\n\", []bool{{true, false}}, qi{n})\n\
                 \tfmt.Printf(\"%{wd}q|%-{wd}q|%#q|%{wd}d|%0{wd}d\\n\", qw{n}, qw{n}, qw{n}, qi{n}, qi{n})\n\
                 \tfmt.Printf(\"%.2q|%.1s|%6.2f\\n\", qw{n}, qw{n}, qf{n})\n\
                 \tfmt.Printf(\"%T|%T|%T|%T\\n\", qb{n}, qi{n}, qc{n}, qy{n})\n\
                 \tfmt.Printf(\"%v|%+v|%#v|%#v\\n\", qb{n}, qp{n}, qb{n}, qm{n})\n\
                 \tfmt.Printf(\"%q|%s|%x\\n\", qc{n}, qs{n}, qc{n})\n"
            )
        }
        // A defined type — `type Weekday int` — is a *distinct* type in Go with
        // its base's representation. Everything it does at run time is the
        // base's behaviour, and the one thing that is not is its name: `%T`
        // prints `main.Weekday`, `%#v` writes the name on a composite, and a
        // method declared on it is reached through it. A frontend that drops the
        // name at the declaration cannot recover any of that, and one that
        // treats the type as opaque loses the base's arithmetic — so both halves
        // are printed for every base kind here.
        37 => {
            uses.defined = true;
            let (a, b) = (rng.int(-9, 40), rng.int(1, 9));
            let w = rng.pick(WORDS);
            let f = rng.pick(&["1.5", "2.25", "-0.75", "10"]);
            format!(
                "\tdi{n} := myInt({a})\n\
                 \tfmt.Printf(\"%T %v %d %q\\n\", di{n}, di{n}, di{n}, di{n})\n\
                 \tfmt.Printf(\"%T %v %d\\n\", di{n}+{b}, di{n}*2, di{n}-{b})\n\
                 \tfmt.Printf(\"%T %v\\n\", di{n}.triple(), int(di{n}))\n\
                 \tvar ds{n} myStr = \"{w}\"\n\
                 \tfmt.Printf(\"%T %q %s %v\\n\", ds{n}, ds{n}, ds{n}, ds{n}+\"z\")\n\
                 \tdf{n} := myFloat({f})\n\
                 \tfmt.Printf(\"%T %v %.2f\\n\", df{n}, df{n}, df{n})\n\
                 \tdb{n} := myBool({a} > 0)\n\
                 \tfmt.Printf(\"%T %v %t\\n\", db{n}, db{n}, db{n})\n\
                 \tdl{n} := mySlice{{{a}, {b}}}\n\
                 \tfmt.Printf(\"%T %v %d %#v\\n\", dl{n}, dl{n}, dl{n}, dl{n})\n\
                 \tfmt.Printf(\"%v %v\\n\", len(dl{n}), dl{n}[0])\n\
                 \tdm{n} := myMap{{\"{w}\": {a}}}\n\
                 \tfmt.Printf(\"%T %v %d\\n\", dm{n}, dm{n}, dm{n}[\"{w}\"])\n\
                 \tda{n} := myArr{{{a}, {b}, {a}}}\n\
                 \tfmt.Printf(\"%T %v\\n\", da{n}, da{n})\n\
                 \tvar dn{n} mySlice\n\tvar dq{n} myMap\n\
                 \tfmt.Printf(\"%T %v %T %v\\n\", dn{n}, dn{n}, dq{n}, dq{n})\n\
                 \tvar dfn{n} myFunc\n\tvar dch{n} myChan\n\
                 \tfmt.Printf(\"%T %T\\n\", dfn{n}, dch{n})\n\
                 \tfmt.Printf(\"%T %v\\n\", []myInt{{myInt({a})}}, []myInt{{myInt({a})}})\n\
                 \tfmt.Printf(\"%T\\n\", map[myStr]myInt{{\"{w}\": myInt({b})}})\n\
                 \tds2{n} := ds{n}\n\tdi2{n} := di{n}\n\
                 \tfmt.Printf(\"%T %T %v %v\\n\", ds2{n}, di2{n}, ds2{n} == ds{n}, di2{n} == di{n})\n\
                 \tfmt.Printf(\"%T %v\\n\", bump(di{n}), bump(di{n}))\n"
            )
        }
        // Interface equality. Go decides it by *dynamic type first, value
        // second*, so two interfaces holding different types are never equal
        // however well the numbers line up — `any(1) == any(1.0)` is false, and
        // so is `any(97) == any(byte(97))`. Comparing two interfaces is the only
        // construct in valid Go that puts two different numeric types (or a
        // number beside a string) under one operator: arithmetic and ordered
        // comparison on mismatched types are compile errors, and an interface is
        // unordered. So this shape is the only reachable probe of the rule.
        //
        // Both answers are printed. A frontend that compared by value alone
        // fails the mismatched pairs, and one that answered a blanket `false`
        // fails the matched ones (`qi == qj`, `qf == qf`, `qz == nil`), so
        // neither can pass by guessing.
        //
        // Every conversion is written from a value inside the narrowest target's
        // range (`byte` tops out at 255): Go rejects an out-of-range constant
        // conversion at compile time, which would make the case a skip rather
        // than a comparison.
        38 => {
            let a = rng.int(1, 120);
            let w = rng.pick(WORDS);
            format!(
                "\tvar qi{n} any = {a}\n\
                 \tvar qf{n} any = float64({a})\n\
                 \tvar qj{n} any = {a}\n\
                 \tvar qs{n} any = \"{a}\"\n\
                 \tvar q64{n} any = int64({a})\n\
                 \tvar q32{n} any = int32({a})\n\
                 \tvar qu{n} any = uint({a})\n\
                 \tvar qb{n} any = byte({a})\n\
                 \tvar qr{n} any = rune({a})\n\
                 \tvar qt{n} any = true\n\
                 \tvar qw{n} any = \"true\"\n\
                 \tvar qz{n} any\n\
                 \tfmt.Println(qi{n} == qf{n}, qi{n} == qj{n}, qi{n} != qf{n}, qi{n} != qj{n})\n\
                 \tfmt.Println(qi{n} == qs{n}, qi{n} == q64{n}, qi{n} == q32{n}, qi{n} == qu{n})\n\
                 \tfmt.Println(qi{n} == qb{n}, qi{n} == qr{n}, qb{n} == qr{n}, q64{n} == qu{n})\n\
                 \tfmt.Println(qt{n} == qw{n}, qf{n} == qf{n}, qs{n} == qs{n}, qt{n} == qt{n})\n\
                 \tfmt.Println(qi{n} == nil, qz{n} == nil, qz{n} != nil, qi{n} == {a})\n\
                 \tfmt.Println(qf{n} == float64({a}), qs{n} == \"{a}\", qi{n} == \"{w}\")\n"
            )
        }
        // new(T) — a zero-valued struct pointer.
        18 => {
            uses.structs = true;
            let x = rng.int(-9, 20);
            format!(
                "\tr{n} := new(pt)\n\tr{n}.x = {x}\n\tfmt.Println(r{n}.x, r{n}.y, r{n}.sum())\n"
            )
        }
        // fmt.Errorf builds an error value; errors.New too.
        19 => {
            uses.errors = true;
            let x = rng.int(-9, 99);
            let w = rng.pick(WORDS);
            format!(
                "\te{n} := fmt.Errorf(\"n=%d %s\", {x}, \"{w}\")\n\tfmt.Println(e{n}, e{n}.Error())\n\tfmt.Println(errors.New(\"{w}\"))\n"
            )
        }
        // defer + recover on a runtime panic (integer divide-by-zero).
        20 => {
            let x = rng.int(1, 99);
            format!(
                "\tfunc() {{\n\t\tdefer func() {{\n\t\t\tif rec := recover(); rec != nil {{\n\t\t\t\tfmt.Println(\"recovered\")\n\t\t\t}}\n\t\t}}()\n\t\tz{n} := 0\n\t\tfmt.Println({x} / z{n})\n\t}}()\n"
            )
        }
        // type switch over an `any` value.
        21 => {
            let (init, _tag) = match rng.below(3) {
                0 => (rng.int(-9, 20).to_string(), "int"),
                1 => (format!("\"{}\"", rng.pick(WORDS)), "string"),
                _ => (rng.pick(&["true", "false"]).to_string(), "bool"),
            };
            format!(
                "\tvar v{n} any = {init}\n\tswitch v{n}.(type) {{\n\tcase int:\n\t\tfmt.Println(\"int\")\n\tcase string:\n\t\tfmt.Println(\"string\")\n\tcase bool:\n\t\tfmt.Println(\"bool\")\n\t}}\n"
            )
        }
        // closure capturing a mutable variable by reference.
        22 => {
            let times = rng.int(1, 5);
            let mut calls = String::new();
            for _ in 0..times {
                calls.push_str(&format!("\tinc{n}()\n"));
            }
            format!("\tc{n} := 0\n\tinc{n} := func() {{ c{n}++ }}\n{calls}\tfmt.Println(c{n})\n")
        }
        // bitwise operators over non-negative ints (i64 == Go's 64-bit int here).
        23 => {
            let (x, y) = (rng.int(0, 255), rng.int(0, 255));
            format!("\tfmt.Println({x}&{y}, {x}|{y}, {x}^{y}, {x}<<2, {x}>>1, {x}&^{y})\n")
        }
        // generic function instantiated at int and float64.
        _ => {
            uses.generic = true;
            let (x, y) = (rng.int(-9, 30), rng.int(-9, 30));
            let (a, b) = (rng.int(0, 12), rng.int(0, 12));
            format!("\tfmt.Println(imax({x}, {y}), imax({a}.5, {b}.5))\n")
        }
    }
}

/// Which optional stdlib packages and top-level preamble declarations a program's
/// blocks reference (`fmt` is always imported), so the import list has no unused
/// entries (Go rejects those) and the preamble emits only what's used.
#[derive(Default)]
struct Uses {
    strings: bool,
    sort: bool,
    math: bool,
    errors: bool,
    structs: bool,
    /// The nested-struct type the value-semantics shape copies through.
    nested: bool,
    /// The array-valued struct and the array-taking helper the array
    /// value-semantics shape copies through.
    arrays: bool,
    /// The variadic helper the over-long-literal shape spreads into, which is
    /// the other way a literal past the arity limit is built.
    bulk: bool,
    generic: bool,
    /// The defer/panic/recover shape's top-level helpers.
    deferred: bool,
    /// The defined types the `%T`-over-a-defined-type shape declares, one per
    /// base kind, plus a method on one of them and a function taking it.
    defined: bool,
}

/// Build a complete, deterministic-output Go program for `seed`.
fn program(seed: u64, only: Option<u64>) -> String {
    let mut rng = Rng::seed(seed);
    let mut uses = Uses::default();
    let nblocks = rng.int(3, 8) as u64;
    let mut body = String::new();
    for i in 0..nblocks {
        body.push_str(&block(&mut rng, i, &mut uses, only));
    }
    let mut imports = vec!["\"fmt\""];
    if uses.errors {
        imports.push("\"errors\"");
    }
    if uses.strings {
        imports.push("\"strings\"");
    }
    if uses.sort {
        imports.push("\"sort\"");
    }
    if uses.math {
        imports.push("\"math\"");
    }
    let import_block = if imports.len() == 1 {
        format!("import {}\n", imports[0])
    } else {
        format!("import (\n\t{}\n)\n", imports.join("\n\t"))
    };
    // Top-level preamble declarations referenced by some blocks — emitted only
    // when used (Go allows unused top-level decls, but keeping programs minimal
    // shrinks divergence repros).
    let mut preamble = String::new();
    if uses.structs {
        preamble.push_str(
            "type pt struct{ x, y int }\n\
             func (p pt) sum() int { return p.x + p.y }\n\
             func (p *pt) scale(k int) { p.x *= k; p.y *= k }\n\n",
        );
    }
    if uses.nested {
        // `valSum` writes to its value receiver before reading it: Go binds that
        // receiver to a copy, so the answer is the same every call and the
        // caller's struct is untouched. `ptrBump` is the pointer-receiver half,
        // which must write through.
        preamble.push_str(
            "type leaf struct{ n int }\n\
             type node struct {\n\tl leaf\n\tk int\n}\n\n\
             func (v node) valSum() int { v.k = 99; v.l.n = 98; return v.k + v.l.n }\n\
             func (v *node) ptrBump(d int) { v.k += d; v.l.n += d }\n\n",
        );
    }
    if uses.arrays {
        // `bumpArr` writes to its array parameter before returning it: Go binds
        // that parameter to a copy, so the caller's array is untouched and the
        // returned one carries the write. `grid` holds arrays by value, so
        // copying a `grid` must copy them — including the `[2]pt`'s structs.
        preamble.push_str(
            "type grid struct {\n\ta [2]int\n\tq [2]pt\n}\n\n\
             func bumpArr(a [3]int, d int) [3]int { a[0] += d; return a }\n\n",
        );
    }
    if uses.bulk {
        // Called both with a spread and with the arguments written out, which
        // is a slice literal built at the call site — the same chunking as the
        // written literal, by a different route.
        preamble.push_str(
            "func sumAll(xs ...int) int {\n\
             \tt := 0\n\tfor _, x := range xs {\n\t\tt += x\n\t}\n\treturn t\n}\n\n",
        );
    }
    if uses.defined {
        // One defined type per base kind. `triple` is a method on a defined
        // *non-struct* type — the receiver is reached through the name, which is
        // the other thing a frontend that erases the declaration loses — and
        // `bump` takes and returns one, so the name has to survive a parameter
        // bind and a return.
        preamble.push_str(
            "type myInt int\n\
             type myStr string\n\
             type myFloat float64\n\
             type myBool bool\n\
             type mySlice []int\n\
             type myMap map[string]int\n\
             type myArr [3]int\n\
             type myFunc func(int) int\n\
             type myChan chan int\n\n\
             func (m myInt) triple() myInt { return m * 3 }\n\n\
             func bump(m myInt) myInt { return m + 1 }\n\n",
        );
    }
    if uses.generic {
        preamble.push_str(
            "func imax[T int | float64](a, b T) T {\n\
             \tif a > b {\n\t\treturn a\n\t}\n\treturn b\n}\n\n",
        );
    }
    if uses.deferred {
        // `noise` is called by a deferred function *before* its `recover()`:
        // Go keeps the panic recoverable across it, so a frontend that treats
        // "a panic is in flight" as a post-call unwind trigger throws the
        // deferred function out before it reaches the `recover()`.
        // `doubled` returns through a `defer` that mutates a named result.
        preamble.push_str(
            "func noise() { fmt.Println(\"noise\") }\n\n\
             func doubled(v int) (r int) {\n\
             \tdefer func() { r *= 2 }()\n\
             \tr = v\n\treturn r\n}\n\n",
        );
    }
    format!("package main\n\n{import_block}\n{preamble}func main() {{\n{body}}}\n")
}

// ── runner ───────────────────────────────────────────────────────────────

/// Wall-clock budget for a single `go run` — either implementation exceeding it
/// is treated as a `<timeout>` result (a caught divergence, so a hang in one
/// case never stalls the whole run).
const CASE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

fn run(bin: &str, src_path: &str) -> (String, bool) {
    let mut child = match Command::new(bin)
        .arg("run")
        .arg(src_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return (format!("<spawn error: {e}>"), false),
    };
    let deadline = std::time::Instant::now() + CASE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out = String::new();
                if let Some(mut so) = child.stdout.take() {
                    use std::io::Read as _;
                    let _ = so.read_to_string(&mut out);
                }
                return (out, status.success());
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return ("<timeout>".to_string(), false);
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(e) => return (format!("<wait error: {e}>"), false),
        }
    }
}

/// This run's private scratch directory, `…/gors_fuzz_<pid>/`.
///
/// The case file used to be named from the seed alone, in the shared temp
/// directory. Seeds start at 0 every run, so two `parity-fuzz` processes in the
/// same checkout wrote — and, after each case, *deleted* — the same paths. A
/// case file removed by the other run while `go run` was still reading it makes
/// the reference fail, which lands in the `skipped` bucket: the run silently
/// stops comparing and still reports a clean rate over whatever survived.
/// Keying the directory on the process id makes two runs unable to see each
/// other's files at all.
fn scratch_dir() -> &'static std::path::Path {
    static DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let d = std::env::temp_dir().join(format!("gors_fuzz_{}", std::process::id()));
        std::fs::create_dir_all(&d).expect("create scratch dir");
        d
    })
}

fn write_tmp(src: &str, tag: u64) -> String {
    let path = scratch_dir().join(format!("case_{tag:016x}.go"));
    let mut f = std::fs::File::create(&path).expect("create temp");
    f.write_all(src.as_bytes()).expect("write temp");
    path.to_string_lossy().into_owned()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut count = 2000u64;
    let mut jobs = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let mut once_seed: Option<u64> = None;
    let mut start_seed = 0u64;
    let mut only: Option<u64> = None;
    let mut ours_override: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--count" => {
                i += 1;
                count = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(count);
            }
            "--jobs" => {
                i += 1;
                jobs = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(jobs);
            }
            "--seed" => {
                i += 1;
                once_seed = args.get(i).and_then(|s| s.parse().ok());
            }
            // `--start N`: begin at seed N (disjoint batches cover distinct seeds
            // across separate runs, each `count` cases wide).
            "--start" => {
                i += 1;
                start_seed = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(0);
            }
            // `--only N`: pin every generated block to statement shape N, so a
            // single shape's divergence rate is measurable rather than diluted
            // across the other 31.
            // `--ours PATH`: run a different go-rs binary than this build's.
            // Pointing it at a binary built from an earlier commit is how a new
            // generator shape is shown to be non-blind — a shape that does not
            // fail against the code from *before* the fix was not testing it.
            "--ours" => {
                i += 1;
                ours_override = args.get(i).cloned();
            }
            "--only" => {
                i += 1;
                only = args
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .filter(|n| *n < SHAPES);
            }
            _ => {}
        }
        i += 1;
    }

    let ours: &str = match &ours_override {
        Some(p) => p,
        None => concat!(env!("CARGO_MANIFEST_DIR"), "/target/debug/go"),
    };
    let oracle = "go";

    // `--seed N --once`: print one program and both outputs, then exit.
    if let Some(seed) = once_seed {
        let src = program(seed, only);
        let path = write_tmp(&src, seed);
        print!("{src}");
        let (g, grc) = run(oracle, &path);
        let (r, rrc) = run(ours, &path);
        println!("--- reference go (ok={grc}) ---\n{g}--- go-rs (ok={rrc}) ---\n{r}");
        let _ = std::fs::remove_file(&path);
        std::process::exit(if g == r && grc == rrc { 0 } else { 1 });
    }

    let next = AtomicU64::new(start_seed);
    let end = start_seed + count;
    let pass = AtomicU64::new(0);
    let fail = AtomicU64::new(0);
    // Cases the reference itself refused (an invalid generated program). Go
    // rejects an unused import or unused variable at *compile* time, so a
    // generator slip produces a program `go` never runs — and since go-rs would
    // usually reject it too, "both produced no stdout and both failed" would be
    // scored as agreement. Two failures agreeing is not a comparison, so these
    // are excluded from the rate and counted on their own: a mode with a large
    // skip count is measuring nothing, which is invisible in a pass rate.
    let skipped = AtomicU64::new(0);
    let divergences: Mutex<Vec<u64>> = Mutex::new(Vec::new());
    let bad_seeds: Mutex<Vec<u64>> = Mutex::new(Vec::new());
    let start = std::time::Instant::now();

    std::thread::scope(|scope| {
        for _ in 0..jobs {
            scope.spawn(|| loop {
                let seed = next.fetch_add(1, Ordering::Relaxed);
                if seed >= end {
                    break;
                }
                let src = program(seed, only);
                let path = write_tmp(&src, seed);
                let (g, grc) = run(oracle, &path);
                // The reference must have actually run the program for the case
                // to be a comparison at all: exit 0 with something on stdout.
                if !grc || g.is_empty() {
                    skipped.fetch_add(1, Ordering::Relaxed);
                    bad_seeds.lock().expect("bad-seed lock").push(seed);
                    let _ = std::fs::remove_file(&path);
                    continue;
                }
                let (r, rrc) = run(ours, &path);
                let _ = std::fs::remove_file(&path);
                if g == r && grc == rrc {
                    pass.fetch_add(1, Ordering::Relaxed);
                } else {
                    fail.fetch_add(1, Ordering::Relaxed);
                    divergences.lock().expect("divergence lock").push(seed);
                }
            });
        }
    });

    let skip = skipped.load(Ordering::Relaxed);
    if skip > 0 {
        let mut bad = bad_seeds.into_inner().expect("bad seeds");
        bad.sort_unstable();
        eprintln!(
            "SKIPPED {skip} case(s) the reference `{oracle}` refused to run \
             (invalid generated program — NOT a comparison). First: {:?}",
            &bad[..bad.len().min(5)]
        );
    }

    let p = pass.load(Ordering::Relaxed);
    let f = fail.load(Ordering::Relaxed);
    let mut divs = divergences.into_inner().unwrap();
    divs.sort_unstable();
    let secs = start.elapsed().as_secs_f64();

    // The rate is over *compared* cases — the ones the reference actually ran.
    // Skipped cases are reported alongside rather than folded in, so a mode
    // that mostly generated programs `go` rejected cannot read as a clean run.
    let compared = p + f;
    println!("\n════════════════════════════════════════════");
    println!("PARITY FUZZ: {p} / {compared} match  ({skip} skipped of {count} generated)",);
    println!(
        "             {jobs} jobs, {:.0}s, {:.0} cases/s, oracle: {oracle}",
        secs,
        count as f64 / secs.max(0.001)
    );
    println!("════════════════════════════════════════════");
    if f > 0 {
        // Persist the full seed list; print the first handful for replay.
        let report = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/target/parity-fuzz-divergences.txt"
        );
        if let Ok(mut file) = std::fs::File::create(report) {
            for s in &divs {
                let _ = writeln!(file, "{s}");
            }
            println!("{} divergent seeds written to {report}", divs.len());
        }
        println!("First divergent seeds (replay with --seed N --once):");
        for s in divs.iter().take(10) {
            println!("  --seed {s} --once");
        }
    }
    // Every generated case must be accounted for. A case the reference refused
    // to run is not a comparison, so a run that skipped some compared fewer
    // programs than it claims to have generated — and exiting 0 on that would
    // let a run that measured nothing read as a clean one. Unaccounted cases
    // (neither compared nor skipped: a lost or clobbered case file) are the
    // same failure and are surfaced by name.
    let missing = count.saturating_sub(p + f + skip);
    if missing > 0 {
        eprintln!(
            "UNACCOUNTED {missing} of {count} generated case(s) were neither compared \
             nor skipped — the run did not execute its own corpus."
        );
    }
    std::process::exit(if f == 0 && skip == 0 && missing == 0 {
        0
    } else {
        1
    });
}
