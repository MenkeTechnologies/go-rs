//! Command-line parsing for the `go` binary.
//!
//! The `go` launcher is subcommand-driven. go-rs wires the subset that makes
//! sense for a single-file `package main` interpreter with a fusevm AOT backend:
//! `run`, `build` (AOT to a native executable), `vet` (compile-check), `env`,
//! `version`, and `help`, plus `--lsp`/`--dap` editor servers and
//! `--dump-tokens`/`--dump-ast`/`--disasm` introspection. A bare `go file.go`
//! is shorthand for `go run file.go`. Module commands (`go mod`, `go get`) are
//! out of scope — go-rs has no module system.

/// The selected subcommand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Command {
    #[default]
    Run,
    /// `go build [-o out] file.go` — AOT-compile to a native executable.
    Build,
    /// `go vet file.go` — parse + compile and report errors (no run).
    Vet,
    /// `go env` — print the Go environment.
    Env,
    Version,
    Help,
    Lsp,
    Dap,
    DumpTokens,
    DumpAst,
    Disasm,
}

/// Parsed command line.
#[derive(Debug, Default)]
pub struct Cli {
    pub cmd: Command,
    /// The `.go` file, if any.
    pub file: Option<String>,
    /// Program arguments after the file (become `os.Args`).
    pub argv: Vec<String>,
    /// `-o <path>` output for `go build`.
    pub out: Option<String>,
    /// Optional topic for `go help <topic>`.
    pub help_topic: Option<String>,
}

/// Parse process args (excluding `argv[0]`).
pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Cli, String> {
    let args: Vec<String> = args.into_iter().collect();
    let mut cli = Cli::default();

    // The first argument may name a subcommand; otherwise it starts a bare
    // `go file.go` run (or a top-level flag).
    let mut idx = 0;
    if let Some(first) = args.first() {
        match first.as_str() {
            "run" => {
                cli.cmd = Command::Run;
                idx = 1;
            }
            "build" => {
                cli.cmd = Command::Build;
                idx = 1;
            }
            "vet" => {
                cli.cmd = Command::Vet;
                idx = 1;
            }
            "env" => {
                cli.cmd = Command::Env;
                idx = 1;
            }
            "version" | "--version" | "-version" => return done(Command::Version),
            "help" | "--help" | "-h" | "-help" | "-?" => {
                cli.cmd = Command::Help;
                cli.help_topic = args.get(1).cloned();
                return Ok(cli);
            }
            "--lsp" => return done(Command::Lsp),
            "--dap" => return done(Command::Dap),
            _ => {}
        }
    }

    // Remaining tokens: flags for the chosen command, then the file + args.
    let mut i = idx;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "-o" if cli.cmd == Command::Build => {
                i += 1;
                cli.out = Some(
                    args.get(i)
                        .cloned()
                        .ok_or_else(|| "go-rs: -o requires an output path".to_string())?,
                );
            }
            "--dump-tokens" => cli.cmd = Command::DumpTokens,
            "--dump-ast" => cli.cmd = Command::DumpAst,
            "--disasm" => cli.cmd = Command::Disasm,
            "--lsp" => cli.cmd = Command::Lsp,
            "--dap" => cli.cmd = Command::Dap,
            "-h" | "--help" => {
                cli.cmd = Command::Help;
            }
            _ if a.starts_with('-') && cli.file.is_none() => {
                return Err(format!("go-rs: unrecognized option `{a}`"))
            }
            _ => {
                if cli.file.is_none() {
                    cli.file = Some(a.clone());
                } else {
                    cli.argv.push(a.clone());
                }
            }
        }
        i += 1;
    }
    Ok(cli)
}

fn done(cmd: Command) -> Result<Cli, String> {
    Ok(Cli {
        cmd,
        ..Default::default()
    })
}

/// `go help` text.
pub const USAGE: &str = "\
usage: go <command> [arguments]

go-rs is a Go frontend on the fusevm bytecode VM. It runs single-file
`package main` programs; it has no module system (no `go get`/`go mod`).

commands:
  run   <file.go> [args]   compile and run a Go program on fusevm
  build [-o out] <file.go> AOT-compile to a standalone native executable
  vet   <file.go>          parse and compile-check; report errors, do not run
  env                      print the Go environment
  version                  print the version banner
  help  [command]          print help (optionally for one command)

a bare `go <file.go>` is shorthand for `go run <file.go>`.

introspection / editor:
  --dump-tokens <file>     print the lexer token stream
  --dump-ast    <file>     print the parsed AST
  --disasm      <file>     print the lowered fusevm bytecode
  --lsp                    Language Server Protocol over stdio
  --dap                    Debug Adapter Protocol over stdio
";

/// Per-command help for `go help <topic>`.
pub fn topic_help(topic: &str) -> Option<&'static str> {
    Some(match topic {
        "run" => "usage: go run <file.go> [args]\n\nCompile the file to fusevm bytecode and run it on the three-tier Cranelift JIT (goroutines/channels run on the cooperative scheduler).\n",
        "build" => "usage: go build [-o output] <file.go>\n\nAOT-compile the file to a standalone native executable via fusevm's ahead-of-time object emitter, linked against the go-rs runtime. Without -o, the output is named after the source file. Note: programs using goroutines/channels/select require the scheduler and must use `go run`.\n",
        "vet" => "usage: go vet <file.go>\n\nParse and compile the file, reporting any lex/parse/compile error, without running it.\n",
        "env" => "usage: go env\n\nPrint the Go environment (GOOS, GOARCH, GOVERSION, …) as go-rs reports it.\n",
        _ => return None,
    })
}
