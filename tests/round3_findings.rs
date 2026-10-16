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
fn struct_field_tags_on_named_fields() {
    // a tag after a named, grouped, or anonymous-struct-typed field is ignored
    let (out, code) = run_file(&script("struct_field_tags"));
    assert_eq!(code, 0);
    assert_eq!(out, "1 {G:0}\n5\n");
}

#[test]
fn float_literal_without_integer_part() {
    // `.5` lexes as a float literal, in constants, expressions and composite literals
    let (out, code) = run_file(&script("float_leading_dot"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "0.5 0.25 5 0.1 1 1.5 -0.75\nfloat64 float64 [0.5 1]\n[2 3] 2\n"
    );
}

#[test]
fn tuple_assignment_evaluates_lhs_index_operands_first() {
    // `i, x[i] = 2, 9` stores into the x[i] of the OLD i
    let (out, code) = run_file(&script("tuple_assign_lhs_index_operands"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "2 [9 2 3]\n2 [9 7 3]\nb map[a:5]\n1 0 [[0 4] [0 0]]\n1 [20 10 30]\n"
    );
}

#[test]
fn print_spaces_operands_by_kind_not_by_rendered_text() {
    // `Print`/`Sprint` separate two operands unless one is of string kind: a
    // `Stringer`/`error` renders to text but is not a string, and a defined
    // string type is
    let (out, code) = run_file(&script("print_spacing_string_kind"));
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "A A|\nC C|\nab|\nssss|\ne e|\nE E|\nP P|\n1 AxA 2|\nA A\ne e\nab1 2c\np1 2q\nA A|\n"
    );
}

/// Run Go source from a temporary file; return (stdout, stderr, exit code).
fn run_src(src: &str) -> (String, String, i32) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("main.go");
    std::fs::write(&path, src).expect("write source");
    let out = Command::new(env!("CARGO_BIN_EXE_go"))
        .arg("run")
        .arg(&path)
        .output()
        .expect("spawn go binary");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

#[test]
fn unused_import_is_a_build_error() {
    let (out, err, code) = run_src(
        "package main\n\nimport (\n\t\"fmt\"\n\t\"os\"\n)\n\nfunc main() { fmt.Println(\"hi\") }\n",
    );
    assert_ne!(code, 0);
    assert_eq!(out, "", "nothing runs when the build fails");
    assert!(err.contains("\"os\" imported and not used"), "stderr: {err}");
}

#[test]
fn import_used_only_as_a_type_or_blank_is_accepted() {
    // `strings` appears only in a qualified type; `_ "os"` binds no name
    let (out, err, code) = run_src(
        "package main\n\nimport (\n\t\"fmt\"\n\t_ \"os\"\n\t\"strings\"\n)\n\nvar sb strings.Builder\n\nfunc main() {\n\tsb.WriteString(\"ok\")\n\tfmt.Println(sb.String())\n}\n",
    );
    assert_eq!((out.as_str(), code), ("ok\n", 0), "stderr: {err}");
}
