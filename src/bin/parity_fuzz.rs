//! Differential parity fuzzer: reference `go run` vs the built go-rs `go run`.
//!
//! Generates grammar-driven, deterministic-output Go programs, runs each through
//! both the reference `go` toolchain and the freshly-built go-rs binary, and
//! reports every case where stdout OR exit status diverges. Each case is a
//! per-index seed so any divergence replays exactly:
//!   `parity-fuzz --seed <N> --once`.
//!
//! Modeled on the sibling frontends' harnesses (rubylang `parity_fuzz`, the
//! zshrs parity fuzzer): splitmix64 PRNG, seed→program generator, byte differ.
//!
//! Determinism invariant: the generator only emits constructs whose output is
//! deterministic and identical across a correct implementation — no goroutines
//! (scheduling order), no maps printed unsorted, no float `%v` shortest-repr
//! (floats are always printed with an explicit `%.4f`, integer `/` uses a
//! guaranteed-nonzero divisor). Pure random bytes would only produce mutual
//! syntax errors that agree and teach nothing.
//!
//! Build: cargo build --bin parity-fuzz
//! Run:   ./target/debug/parity-fuzz --count 2000

use std::io::Write as _;
use std::process::{Command, Stdio};

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
}

// ── generators ─────────────────────────────────────────────────────────────

/// A generated integer expression (small, so overflow never differs).
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
        4 => format!("({} %% {})", int_expr(rng, depth - 1), rng.int(1, 9)),
        _ => rng.int(-9, 20).to_string(),
    }
}

/// A generated float expression, printed with a fixed `%.4f` so both format
/// identically (avoids Go-`%v` vs Rust-Display shortest-repr differences).
fn float_expr(rng: &mut Rng, depth: u32) -> String {
    if depth == 0 || rng.below(3) == 0 {
        return format!("{}.{}", rng.int(0, 12), rng.below(1000));
    }
    match rng.below(4) {
        0 => format!(
            "({} + {})",
            float_expr(rng, depth - 1),
            float_expr(rng, depth - 1)
        ),
        1 => format!(
            "({} - {})",
            float_expr(rng, depth - 1),
            float_expr(rng, depth - 1)
        ),
        2 => format!(
            "({} * {})",
            float_expr(rng, depth - 1),
            float_expr(rng, depth - 1)
        ),
        // Nonzero divisor.
        _ => format!(
            "({} / {}.{})",
            float_expr(rng, depth - 1),
            rng.int(1, 9),
            1 + rng.below(9)
        ),
    }
}

/// A generated boolean expression.
fn bool_expr(rng: &mut Rng, depth: u32) -> String {
    if depth == 0 || rng.below(2) == 0 {
        let ops = ["<", "<=", ">", ">=", "==", "!="];
        let op = ops[rng.below(ops.len() as u64) as usize];
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

/// A generated string expression (concatenation of literals).
fn str_expr(rng: &mut Rng, depth: u32) -> String {
    let words = ["go", "rs", "fuse", "vm", "abc", "xyz", "", "hello"];
    if depth == 0 || rng.below(2) == 0 {
        return format!("\"{}\"", words[rng.below(words.len() as u64) as usize]);
    }
    format!(
        "({} + {})",
        str_expr(rng, depth - 1),
        str_expr(rng, depth - 1)
    )
}

/// Build a complete, deterministic-output Go program for `seed`.
fn program(seed: u64) -> String {
    let mut rng = Rng::seed(seed);
    let mut body = String::new();
    // A handful of Printf lines exercising each value kind.
    body.push_str(&format!(
        "\tfmt.Printf(\"%d %d %d\\n\", {}, {}, {})\n",
        int_expr(&mut rng, 3),
        int_expr(&mut rng, 3),
        int_expr(&mut rng, 3)
    ));
    body.push_str(&format!(
        "\tfmt.Printf(\"%.4f %.4f\\n\", {}, {})\n",
        float_expr(&mut rng, 2),
        float_expr(&mut rng, 2)
    ));
    body.push_str(&format!(
        "\tfmt.Println({}, {})\n",
        bool_expr(&mut rng, 2),
        bool_expr(&mut rng, 2)
    ));
    body.push_str(&format!("\tfmt.Println({})\n", str_expr(&mut rng, 2)));
    // A string comparison (lexicographic ordering parity).
    body.push_str(&format!(
        "\tfmt.Println({} < {})\n",
        str_expr(&mut rng, 1),
        str_expr(&mut rng, 1)
    ));
    format!("package main\n\nimport \"fmt\"\n\nfunc main() {{\n{body}}}\n")
}

// ── runner ───────────────────────────────────────────────────────────────

fn run(bin: &str, args: &[&str], src_path: &str) -> (String, bool) {
    let out = Command::new(bin)
        .args(args)
        .arg(src_path)
        .stdin(Stdio::null())
        .output();
    match out {
        Ok(o) => (
            String::from_utf8_lossy(&o.stdout).into_owned(),
            o.status.success(),
        ),
        Err(e) => (format!("<spawn error: {e}>"), false),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut count = 2000u64;
    let mut once_seed: Option<u64> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--count" => {
                i += 1;
                count = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(count);
            }
            "--seed" => {
                i += 1;
                once_seed = args.get(i).and_then(|s| s.parse().ok());
            }
            "--once" => {}
            _ => {}
        }
        i += 1;
    }

    let ours = concat!(env!("CARGO_MANIFEST_DIR"), "/target/debug/go");
    let oracle = "go";

    // `--seed N --once`: print one program and both outputs, then exit.
    if let Some(seed) = once_seed {
        let src = program(seed);
        let path = write_tmp(&src);
        print!("{src}");
        let (g, grc) = run(oracle, &["run"], &path);
        let (r, rrc) = run(ours, &["run"], &path);
        println!("--- reference go (rc ok={grc}) ---\n{g}--- go-rs (rc ok={rrc}) ---\n{r}");
        let _ = std::fs::remove_file(&path);
        std::process::exit(if g == r && grc == rrc { 0 } else { 1 });
    }

    let mut pass = 0u64;
    let mut fail = 0u64;
    let mut first_divergences: Vec<u64> = Vec::new();
    for seed in 0..count {
        let src = program(seed);
        let path = write_tmp(&src);
        let (g, grc) = run(oracle, &["run"], &path);
        let (r, rrc) = run(ours, &["run"], &path);
        let _ = std::fs::remove_file(&path);
        if g == r && grc == rrc {
            pass += 1;
        } else {
            fail += 1;
            if first_divergences.len() < 10 {
                first_divergences.push(seed);
            }
        }
    }
    println!("\n════════════════════════════════════════════");
    println!("PARITY FUZZ: {pass} / {count} match  (oracle: {oracle})");
    println!("════════════════════════════════════════════");
    if fail > 0 {
        println!("First divergent seeds (replay with --seed N --once):");
        for s in &first_divergences {
            println!("  --seed {s} --once");
        }
    }
    std::process::exit(if fail == 0 { 0 } else { 1 });
}

fn write_tmp(src: &str) -> String {
    // A per-process, per-content temp file (no external tempfile dep in the bin).
    let mut hasher = 1469598103934665603u64; // FNV-1a
    for b in src.bytes() {
        hasher ^= b as u64;
        hasher = hasher.wrapping_mul(1099511628211);
    }
    let path = std::env::temp_dir().join(format!("gors_fuzz_{hasher:016x}.go"));
    let mut f = std::fs::File::create(&path).expect("create temp");
    f.write_all(src.as_bytes()).expect("write temp");
    path.to_string_lossy().into_owned()
}
