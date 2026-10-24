//! Regressions found by differential probing against the reference `go`.
//! Each program is the `parity-scripts/` file that reproduces it; the expected
//! text is the verbatim stdout of `go run` on that file, so the tests need no
//! `go` toolchain.

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
fn pointer_to_defined_scalar_is_its_own_dynamic_type() {
    // `&c` of a `type Celsius float64` enters an interface as `*Celsius`: `%T`,
    // `case *Celsius:`, assertions, `errors.As`, the `*T` method set (value
    // methods read through the pointer) and `fmt.Sscan`'s reflect fallback
    let (out, code) = run_file(&script("pointer_to_defined_scalar"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "*main.Celsius *main.Celsius main.Celsius\n\
         ptr 3.0C val 3.0C\n\
         false true 3.0C\n\
         21.5C 21.5C 21.5C\n\
         *main.Celsius\n\
         5 count=5\n\
         count=5 count=5 5\n\
         6\n\
         *Count 7\n\
         Count 7\n\
         false true false\n\
         11\n\
         *main.Count *main.Count\n\
         1\n\
         count=7 count=1 [count=7 count=1]\n\
         true false\n\
         *main.Code code 7\n\
         true 7\n\
         4 <nil> hello true -7 4.2C\n\
         0 integer overflow on token 300 -7\n\
         0 expected integer 7\n\
         2 <nil> 12 2.5C\n\
         0 type not a pointer: main.Name\n"
    );
}

#[test]
fn pointer_type_conversion_keeps_the_target_type() {
    // `(*stringReader)(&str)` is a `*stringReader` whose `Read` rebinds `str`;
    // `(*Celsius)(&f)` writes `f`; `fmt.Sscan` reads its string the same way
    let (out, code) = run_file(&script("pointer_type_conversion"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "3 <nil> \"hel\" \"lo\"\n\
         2 <nil> \"lo\" \"\"\n\
         0 EOF \"\" \"\"\n\
         *main.stringReader\n\
         21.5C 21.5C\n\
         30\n\
         *main.Celsius *main.Celsius\n\
         30.0C\n\
         8\n\
         2 <nil> alpha beta\n"
    );
}
