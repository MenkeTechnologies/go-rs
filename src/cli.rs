//! Command-line parsing for the `go` binary.
//!
//! The `go` launcher is subcommand-driven; slice 1 wires `go run <file.go>`
//! (plus a bare `go <file.go>` shorthand) and a small set of introspection
//! flags. The full toolchain grammar (`go build`, `go test`, module commands)
//! grows in later waves; unknown options error rather than being silently
//! ignored.

/// Parsed command line.
#[derive(Debug, Default)]
pub struct Cli {
    /// The `.go` file to run, if any.
    pub file: Option<String>,
    /// Program arguments after the file (become `os.Args` — unused in slice 1).
    pub argv: Vec<String>,
    pub show_version: bool,
    pub show_help: bool,
    /// `--dump-tokens` — print the lexer token stream and exit.
    pub dump_tokens: bool,
    /// `--dump-ast` — print the parsed AST and exit.
    pub dump_ast: bool,
    /// `--disasm` — print the lowered fusevm bytecode and exit.
    pub disasm: bool,
    /// `--lsp` — speak the Language Server Protocol over stdio.
    pub lsp: bool,
    /// `--dap` — speak the Debug Adapter Protocol over stdio.
    pub dap: bool,
}

/// Parse process args (excluding `argv[0]`).
pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Cli, String> {
    let mut cli = Cli::default();
    let mut saw_run = false;
    for a in args {
        match a.as_str() {
            "version" | "--version" | "-version" if cli.file.is_none() => cli.show_version = true,
            "--help" | "-h" | "-help" | "help" => cli.show_help = true,
            // `go run FILE` — the `run` subcommand is consumed, the file follows.
            "run" if !saw_run && cli.file.is_none() => saw_run = true,
            "--dump-tokens" => cli.dump_tokens = true,
            "--dump-ast" => cli.dump_ast = true,
            "--disasm" => cli.disasm = true,
            "--lsp" => cli.lsp = true,
            "--dap" => cli.dap = true,
            _ if a.starts_with('-') && cli.file.is_none() => {
                return Err(format!("go-rs: unrecognized option `{a}`"))
            }
            _ => {
                if cli.file.is_none() {
                    cli.file = Some(a);
                } else {
                    cli.argv.push(a);
                }
            }
        }
    }
    Ok(cli)
}

/// `go --help` text.
pub const USAGE: &str = "\
usage: go [run] <file.go> [args...]

commands:
  run <file.go>         compile and run a Go program on fusevm
  version               print the version banner and exit
  help                  print this help and exit

options:
  --dump-tokens         print the lexer token stream and exit
  --dump-ast            print the parsed AST and exit
  --disasm              print the lowered fusevm bytecode and exit
  --lsp                 speak the Language Server Protocol over stdio
  --dap                 speak the Debug Adapter Protocol over stdio
";
