use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use awiki_deamon::{run_command_json, DaemonCommand};

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let command = parse_args(std::env::args().skip(1))?;
    let output = run_command_json(command)?;
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<DaemonCommand> {
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        return usage_error();
    };

    let mut state_root = None;
    let mut agent_did = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--agent-did" => {
                let value = args.next().context("--agent-did requires a DID argument")?;
                agent_did = Some(value);
            }
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
        "agent-list" => Ok(DaemonCommand::AgentList { state_root }),
        "agent-status" => {
            let agent_did = agent_did
                .or_else(|| std::env::var("AWIKI_DAEMON_AGENT_DID").ok())
                .context("--agent-did or AWIKI_DAEMON_AGENT_DID is required")?;
            Ok(DaemonCommand::AgentStatus {
                state_root,
                agent_did,
            })
        }
        "foreground" => Ok(DaemonCommand::Foreground { state_root }),
        "init-state" => Ok(DaemonCommand::InitState { state_root }),
        "runtime-list" => Ok(DaemonCommand::RuntimeList { state_root }),
        "status" => Ok(DaemonCommand::Status { state_root }),
        other => bail!("unknown command: {other}"),
    }
}

fn usage_error<T>() -> Result<T> {
    bail!("usage: awiki-deamon <foreground|init-state|status|agent-list|agent-status|runtime-list> --state-root <path> [--agent-did <did>]")
}
