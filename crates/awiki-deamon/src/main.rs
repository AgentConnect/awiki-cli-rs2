use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use awiki_deamon::{run_command, DaemonCommand};

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let command = parse_args(std::env::args().skip(1))?;
    let status = run_command(command)?;
    println!("{}", serde_json::to_string_pretty(&status)?);
    Ok(())
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<DaemonCommand> {
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        return usage_error();
    };

    let mut state_root = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--state-root" => {
                let value = args
                    .next()
                    .context("--state-root requires a path argument")?;
                state_root = Some(PathBuf::from(value));
            }
            "--help" | "-h" => return usage_error(),
            other => bail!("unknown argument: {other}"),
        }
    }

    let Some(state_root) = state_root else {
        bail!("--state-root is required");
    };

    match command.as_str() {
        "foreground" => Ok(DaemonCommand::Foreground { state_root }),
        "init-state" => Ok(DaemonCommand::InitState { state_root }),
        "status" => Ok(DaemonCommand::Status { state_root }),
        other => bail!("unknown command: {other}"),
    }
}

fn usage_error<T>() -> Result<T> {
    bail!("usage: awiki-deamon <foreground|init-state|status> --state-root <path>")
}
