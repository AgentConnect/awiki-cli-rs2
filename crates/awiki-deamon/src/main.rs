use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use awiki_deamon::{
    daemon_cli::SetupDaemonAgentOptions, foreground::ForegroundOptions, run_command_json,
    DaemonCommand,
};

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
    let mut controller_did = None;
    let mut handle = None;
    let mut registration_token = None;
    let mut poll_interval_ms = None;
    let mut max_runtime_ms = None;
    let mut max_processed_messages = None;
    let mut ready_file = None;
    let mut agent_jwt_token = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--agent-did" => {
                let value = args.next().context("--agent-did requires a DID argument")?;
                agent_did = Some(value);
            }
            "--agent-jwt-token" => {
                let value = args
                    .next()
                    .context("--agent-jwt-token requires a token argument")?;
                agent_jwt_token = Some(value);
            }
            "--controller-did" => {
                let value = args
                    .next()
                    .context("--controller-did requires a DID argument")?;
                controller_did = Some(value);
            }
            "--handle" => {
                let value = args.next().context("--handle requires a value")?;
                handle = Some(value);
            }
            "--max-processed-messages" => {
                let value = args
                    .next()
                    .context("--max-processed-messages requires a number")?;
                max_processed_messages = Some(value.parse()?);
            }
            "--max-runtime-ms" => {
                let value = args.next().context("--max-runtime-ms requires a number")?;
                max_runtime_ms = Some(value.parse()?);
            }
            "--poll-interval-ms" => {
                let value = args
                    .next()
                    .context("--poll-interval-ms requires a number")?;
                poll_interval_ms = Some(value.parse()?);
            }
            "--ready-file" => {
                let value = args
                    .next()
                    .context("--ready-file requires a path argument")?;
                ready_file = Some(PathBuf::from(value));
            }
            "--registration-token" => {
                let value = args
                    .next()
                    .context("--registration-token requires a token argument")?;
                registration_token = Some(value);
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
        "foreground" => {
            let mut options = ForegroundOptions::new(state_root);
            if let Some(value) = poll_interval_ms {
                options.poll_interval_ms = value;
            }
            options.max_runtime_ms = max_runtime_ms;
            options.max_processed_messages = max_processed_messages;
            options.ready_file = ready_file;
            options.agent_jwt_token =
                agent_jwt_token.or_else(|| std::env::var("AWIKI_DAEMON_AGENT_JWT_TOKEN").ok());
            Ok(DaemonCommand::Foreground { options })
        }
        "init-state" => Ok(DaemonCommand::InitState { state_root }),
        "runtime-list" => Ok(DaemonCommand::RuntimeList { state_root }),
        "setup-daemon-agent" => {
            let handle = handle
                .or_else(|| std::env::var("AWIKI_DAEMON_HANDLE").ok())
                .context("--handle or AWIKI_DAEMON_HANDLE is required")?;
            let controller_did = controller_did
                .or_else(|| std::env::var("AWIKI_DAEMON_CONTROLLER_DID").ok())
                .context("--controller-did or AWIKI_DAEMON_CONTROLLER_DID is required")?;
            let registration_token = registration_token
                .or_else(|| std::env::var("AWIKI_DAEMON_REGISTRATION_TOKEN").ok())
                .context("--registration-token or AWIKI_DAEMON_REGISTRATION_TOKEN is required")?;
            Ok(DaemonCommand::SetupDaemonAgent {
                state_root,
                options: SetupDaemonAgentOptions {
                    handle,
                    controller_did,
                    registration_token,
                },
            })
        }
        "status" => Ok(DaemonCommand::Status { state_root }),
        other => bail!("unknown command: {other}"),
    }
}

fn usage_error<T>() -> Result<T> {
    bail!("usage: awiki-deamon <foreground|init-state|status|agent-list|agent-status|runtime-list|setup-daemon-agent> --state-root <path> [--agent-did <did>] [setup-daemon-agent: --handle <handle> --controller-did <did> --registration-token <token>] [foreground: --ready-file <path> --max-runtime-ms <ms> --max-processed-messages <n> --poll-interval-ms <ms> --agent-jwt-token <token>]")
}
