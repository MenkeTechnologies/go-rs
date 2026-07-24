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
//! scheduling order, floats printed with an explicit `%.4f` (never `%v`
//! shortest-repr), integer `/` and `%` with a guaranteed-nonzero divisor, and
//! maps printed via `fmt` (which sorts keys on both sides). Pure random bytes
//! would only produce mutual syntax errors that agree and teach nothing.
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

/// Emit a random block of statements. `n` is a fresh var-name suffix.
fn block(rng: &mut Rng, n: u64, uses: &mut Uses) -> String {
    match rng.below(25) {
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
    generic: bool,
}

/// Build a complete, deterministic-output Go program for `seed`.
fn program(seed: u64) -> String {
    let mut rng = Rng::seed(seed);
    let mut uses = Uses::default();
    let nblocks = rng.int(3, 8) as u64;
    let mut body = String::new();
    for i in 0..nblocks {
        body.push_str(&block(&mut rng, i, &mut uses));
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
    if uses.generic {
        preamble.push_str(
            "func imax[T int | float64](a, b T) T {\n\
             \tif a > b {\n\t\treturn a\n\t}\n\treturn b\n}\n\n",
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

fn write_tmp(src: &str, tag: u64) -> String {
    let path = std::env::temp_dir().join(format!("gors_fuzz_{tag:016x}.go"));
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
            _ => {}
        }
        i += 1;
    }

    let ours = concat!(env!("CARGO_MANIFEST_DIR"), "/target/debug/go");
    let oracle = "go";

    // `--seed N --once`: print one program and both outputs, then exit.
    if let Some(seed) = once_seed {
        let src = program(seed);
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
    let divergences: Mutex<Vec<u64>> = Mutex::new(Vec::new());
    let start = std::time::Instant::now();

    std::thread::scope(|scope| {
        for _ in 0..jobs {
            scope.spawn(|| loop {
                let seed = next.fetch_add(1, Ordering::Relaxed);
                if seed >= end {
                    break;
                }
                let src = program(seed);
                let path = write_tmp(&src, seed);
                let (g, grc) = run(oracle, &path);
                let (r, rrc) = run(ours, &path);
                let _ = std::fs::remove_file(&path);
                if g == r && grc == rrc {
                    pass.fetch_add(1, Ordering::Relaxed);
                } else {
                    fail.fetch_add(1, Ordering::Relaxed);
                    divergences.lock().unwrap().push(seed);
                }
            });
        }
    });

    let p = pass.load(Ordering::Relaxed);
    let f = fail.load(Ordering::Relaxed);
    let mut divs = divergences.into_inner().unwrap();
    divs.sort_unstable();
    let secs = start.elapsed().as_secs_f64();

    println!("\n════════════════════════════════════════════");
    println!(
        "PARITY FUZZ: {p} / {count} match  ({jobs} jobs, {:.0}s, {:.0} cases/s, oracle: {oracle})",
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
    std::process::exit(if f == 0 { 0 } else { 1 });
}
