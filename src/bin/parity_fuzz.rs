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

/// A float expression over the given (runtime) variables. Leaves are variables,
/// NOT literals: Go evaluates a constant float expression with arbitrary
/// precision and rounds once, but any variable forces the runtime `f64` path
/// (double-rounded) that go-rs implements — so combining variables keeps both
/// sides on the same footing. (The constant-folding difference is a documented
/// go-rs limitation, tested here only via single literals, never arithmetic.)
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
    match rng.below(10) {
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
        // math stdlib (fixed precision so both format identically)
        _ => {
            uses.math = true;
            let (decl, vars) = float_vars(rng, n);
            let f = float_expr(rng, &vars, 1);
            let g = float_expr(rng, &vars, 1);
            format!(
                "{decl}\tfmt.Printf(\"%.4f %.4f %.0f\\n\", math.Sqrt({f}), math.Abs(-({g})), math.Floor({f}))\n"
            )
        }
    }
}

/// Which optional stdlib packages a program's blocks reference (`fmt` is always
/// imported), so the import list has no unused entries (Go rejects those).
#[derive(Default)]
struct Uses {
    strings: bool,
    sort: bool,
    math: bool,
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
    format!("package main\n\n{import_block}\nfunc main() {{\n{body}}}\n")
}

// ── runner ───────────────────────────────────────────────────────────────

fn run(bin: &str, src_path: &str) -> (String, bool) {
    match Command::new(bin)
        .arg("run")
        .arg(src_path)
        .stdin(Stdio::null())
        .output()
    {
        Ok(o) => (
            String::from_utf8_lossy(&o.stdout).into_owned(),
            o.status.success(),
        ),
        Err(e) => (format!("<spawn error: {e}>"), false),
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

    let next = AtomicU64::new(0);
    let pass = AtomicU64::new(0);
    let fail = AtomicU64::new(0);
    let divergences: Mutex<Vec<u64>> = Mutex::new(Vec::new());
    let start = std::time::Instant::now();

    std::thread::scope(|scope| {
        for _ in 0..jobs {
            scope.spawn(|| loop {
                let seed = next.fetch_add(1, Ordering::Relaxed);
                if seed >= count {
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
