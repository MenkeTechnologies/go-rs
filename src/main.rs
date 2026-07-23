//! The `go` binary entry point.
//!
//! Dispatches the go-rs subcommand set — `run` / `build` / `vet` / `env` /
//! `version` / `help`, the `--lsp`/`--dap` servers, and the
//! `--dump-tokens`/`--dump-ast`/`--disasm` introspection flags. Errors go to
//! stderr in terse `go-rs: <reason>` form; nothing else is printed.

use gors::cli::Command;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = match gors::cli::parse(std::env::args().skip(1)) {
        Ok(c) => c,
        Err(e) => return fail(&e),
    };

    match cli.cmd {
        Command::Version => {
            println!("{}", gors::version_banner());
            ExitCode::SUCCESS
        }
        Command::Help => {
            match cli.help_topic.as_deref().and_then(gors::cli::topic_help) {
                Some(t) => print!("{t}"),
                None => print!("{}", gors::cli::USAGE),
            }
            ExitCode::SUCCESS
        }
        Command::Env => {
            print!("{}", gors::env_report());
            ExitCode::SUCCESS
        }
        Command::Lsp => match gors::lsp::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => fail(&e),
        },
        Command::Dap => match gors::dap::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => fail(&e),
        },
        Command::Run
        | Command::Build
        | Command::Vet
        | Command::DumpTokens
        | Command::DumpAst
        | Command::Disasm => run_file_cmd(&cli),
    }
}

/// Handle the subcommands that need a source file.
fn run_file_cmd(cli: &gors::cli::Cli) -> ExitCode {
    let Some(file) = cli.file.clone() else {
        return fail("no input file (try `go help`)");
    };
    let src = match std::fs::read_to_string(&file) {
        Ok(s) => s,
        Err(e) => return fail(&format!("cannot read {file}: {e}")),
    };

    match cli.cmd {
        Command::DumpTokens => finish(dump_tokens(&src)),
        Command::DumpAst => finish(dump_ast(&src)),
        Command::Disasm => finish(gors::disassemble(&src)),
        Command::Vet => match gors::vet(&src) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => fail(&e),
        },
        Command::Build => {
            let out = cli
                .out
                .clone()
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| gors::aot_native::default_output(&file));
            match gors::aot_native::build(&src, &out) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => fail(&e),
            }
        }
        _ => match gors::run_str(&src) {
            Ok(_) => ExitCode::SUCCESS,
            Err(e) => fail(&e),
        },
    }
}

fn dump_tokens(src: &str) -> Result<String, String> {
    let toks = gors::lexer::lex(src)?;
    let mut out = String::new();
    for t in toks {
        out.push_str(&format!("{:>4}  {:?}\n", t.line, t.kind));
    }
    Ok(out)
}

fn dump_ast(src: &str) -> Result<String, String> {
    let prog = gors::parse(src)?;
    Ok(format!("{prog:#?}\n"))
}

fn finish(r: Result<String, String>) -> ExitCode {
    match r {
        Ok(s) => {
            print!("{s}");
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e),
    }
}

fn fail(msg: &str) -> ExitCode {
    let msg = msg.strip_prefix("go-rs: ").unwrap_or(msg);
    eprintln!("go-rs: {msg}");
    ExitCode::FAILURE
}
