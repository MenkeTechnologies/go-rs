//! Regressions for divergences the differential harness found against the
//! reference `go`. Each program is the `parity-scripts/` file that reproduces
//! it; the expected text is the verbatim stdout of `go run` on that file, so
//! the tests need no `go` toolchain.

use std::process::Command;

/// Run a program file through the built `go` binary; return (stdout, exit code).
fn run_file(path: &str) -> (String, i32) {
    let out = Command::new(env!("CARGO_BIN_EXE_go"))
        .arg("run")
        .arg(path)
        .output()
        .expect("spawn go binary");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

fn script(name: &str) -> String {
    format!("{}/parity-scripts/{name}.go", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn function_literal_named_results_bare_return_and_defer() {
    let (out, code) = run_file(&script("named_results_func_literal"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "6 0 9\n4 true\n2\n14 d\n-7 n!\n recovered: assignment to entry in nil map\n0  0 false [] <nil>\n"
    );
}

#[test]
fn fields_promote_through_an_embedded_pointer() {
    let (out, code) = run_file(&script("embedded_pointer_promotion"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "3 rex ball 3\n4 max max\n14 max rope 14 rope 2\n7 true\n"
    );
}

#[test]
fn loop_variable_is_not_the_later_top_level_variable_of_the_same_name() {
    let (out, code) = run_file(&script("loop_var_vs_later_top_level"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "0 1 2 100\nlater\nrange defer 1\nrange defer 0\ndefer 20 2\ndefer 10 1\ndefer 0 0\n"
    );
}

#[test]
fn sharp_v_writes_bytes_unpadded_and_arrays_under_their_type() {
    let (out, code) = run_file(&script("fmt_sharp_v_bytes"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "[]byte{0x1, 0x2, 0xff}\n[3]uint8{0x9, 0x0, 0x0}\n[9 0 0] 090000\n\
         [2]uint8{0x1, 0x2} [0]int{}\n\
         [2][]uint8{[]uint8{0x1}, []uint8{0x2}} struct { B [2]uint8 }{B:[2]uint8{0x3, 0x4}}\n"
    );
}

#[test]
fn identifiers_may_be_unicode() {
    let (out, code) = run_file(&script("unicode_identifiers"));
    assert_eq!(code, 0);
    assert_eq!(out, "3.14 wörld 6 12 {3 4}\n0 1 \n");
}

#[test]
fn shortest_float_ties_take_the_even_digit() {
    let (out, code) = run_file(&script("float_shortest_even_tie"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "0.10101699829101562 -0.10101699829101562\n\
         0.10101699829101562 1.0101699829101562e-01 0.10101699829101562\n\
         0.10101699829101562 0.10101699829101562 0.1010169982910156 0.10101699829101562 0.101016998291016\n\
         0.3 0.3\n2.675 2.675\n1e+23 1e+23\n8.41e+21 8.41e+21\n5e-324 0\n\
         1.2345678901234568e+17 1.2345679e+17\n4.02569325e+06 4.0256932e+06\n"
    );
}

#[test]
fn parse_float_follows_the_go_literal_grammar() {
    let (out, code) = run_file(&script("strconv_parse_float_grammar"));
    assert_eq!(code, 0);
    let line = |l: &str| assert!(out.lines().any(|o| o == l), "missing line: {l}\n{out}");
    // Hex floats, with the mandatory `p` exponent, and `_` separators.
    line("\"0x1p-2\" 0.25 <nil> 0.25 <nil>");
    line("\"0x1.8p1\" 3 <nil> 3 <nil>");
    line("\"0x.8p0\" 0.5 <nil> 0.5 <nil>");
    line("\"0x_1p0\" 1 <nil> 1 <nil>");
    line("\"1_0.5\" 10.5 <nil> 10.5 <nil>");
    line("\"1e+0_1\" 10 <nil> 10 <nil>");
    line(
        "\"0x10\" 0 strconv.ParseFloat: parsing \"0x10\": invalid syntax \
         0 strconv.ParseFloat: parsing \"0x10\": invalid syntax",
    );
    line(
        "\"1__0\" 0 strconv.ParseFloat: parsing \"1__0\": invalid syntax \
         0 strconv.ParseFloat: parsing \"1__0\": invalid syntax",
    );
    // Specials: `infinity` is not a range error, a signed NaN is a syntax error.
    line("\"+INFINITY\" +Inf <nil> +Inf <nil>");
    line("\"NaN\" NaN <nil> NaN <nil>");
    line(
        "\"-nan\" 0 strconv.ParseFloat: parsing \"-nan\": invalid syntax \
         0 strconv.ParseFloat: parsing \"-nan\": invalid syntax",
    );
    // bitSize 32 rounds to float32 and overflows at its own range.
    line("\"0.1\" 0.1 <nil> 0.10000000149011612 <nil>");
    line(
        "\"3.4e39\" 3.4e+39 <nil> +Inf strconv.ParseFloat: parsing \"3.4e39\": value out of range",
    );
}

#[test]
fn os_exit_as_a_function_value_exits_without_running_defers() {
    let (out, code) = run_file(&script("os_exit_func_value"));
    assert_eq!(out, "before\n");
    assert_eq!(code, 3);
}
