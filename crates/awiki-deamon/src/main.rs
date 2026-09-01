use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use awiki_deamon::{
    cli_wrapper::{
        run_wrapper_command, runtime_token_from_env_or_arg, socket_from_env_or_arg,
        CliWrapperCommand,
    },
    daemon_cli::{InstallOptions, SetupDaemonAgentOptions},
    foreground::ForegroundOptions,
    run_command_json,
    service::ServiceAction,
    DaemonCommand, DaemonConfig,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    if is_runtime_wrapper_invocation() {
        let response = run_runtime_wrapper(std::env::args().skip(1))?;
        println!("{}", serde_json::to_string(&response)?);
        if !response.ok {
            std::process::exit(2);
        }
        return Ok(());
    }
    if let Some(command) = std::env::args().nth(1) {
        match command.as_str() {
            "__runtime-wrapper" => {
                let response = run_runtime_wrapper(std::env::args().skip(2))?;
                println!("{}", serde_json::to_string(&response)?);
                if !response.ok {
                    std::process::exit(2);
                }
                return Ok(());
            }
            "__self-check" => {
                run_self_check(std::env::args().skip(2))?;
                return Ok(());
            }
            _ => {}
        }
    }
    let command = parse_args(std::env::args().skip(1))?;
    let output = run_command_json(command)?;
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn run_self_check(args: impl IntoIterator<Item = String>) -> Result<()> {
    let mut args = args.into_iter();
    let mut expected_version = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--expected-version" => {
                expected_version =
                    Some(args.next().context("--expected-version requires a value")?);
            }
            "--help" | "-h" => return self_check_usage_error(),
            other => bail!("unknown self-check argument: {other}"),
        }
    }
    if let Some(expected) = expected_version {
        let expected = expected.trim().trim_start_matches('v');
        let actual = env!("CARGO_PKG_VERSION").trim().trim_start_matches('v');
        if expected != actual {
            bail!("daemon self-check version mismatch: expected {expected}, got {actual}");
        }
    }
    println!(
        "{{\"ok\":true,\"name\":\"awiki-deamon\",\"version\":\"{}\"}}",
        env!("CARGO_PKG_VERSION")
    );
    Ok(())
}

fn is_runtime_wrapper_invocation() -> bool {
    std::env::args()
        .next()
        .and_then(|path| {
            std::path::Path::new(&path)
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        })
        .is_some_and(|name| name == "awiki-deamon-runtime")
}

fn run_runtime_wrapper(
    args: impl IntoIterator<Item = String>,
) -> Result<awiki_deamon::local_rpc::RuntimeRpcResponse> {
    let command = parse_runtime_wrapper_args(args)?;
    run_wrapper_command(command)
}

fn parse_runtime_wrapper_args(args: impl IntoIterator<Item = String>) -> Result<CliWrapperCommand> {
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        return runtime_wrapper_usage_error();
    };
    let mut socket = None;
    let mut token = None;
    let mut to = None;
    let mut group = None;
    let mut text = None;
    let mut file_path = None;
    let mut display_filename = None;
    let mut mime_type = None;
    let mut caption = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--socket" => {
                socket = Some(PathBuf::from(
                    args.next().context("--socket requires a path")?,
                ));
            }
            "--token" => {
                token = Some(args.next().context("--token requires a value")?);
            }
            "--to" => {
                to = Some(args.next().context("--to requires a direct recipient")?);
            }
            "--to-handle" => {
                to = Some(
                    args.next()
                        .context("--to-handle requires a direct recipient")?,
                );
            }
            "--group" => {
                group = Some(args.next().context("--group requires a group")?);
            }
            "--text" => {
                text = Some(args.next().context("--text requires a value")?);
            }
            "--file" => {
                file_path = Some(args.next().context("--file requires a path")?);
            }
            "--display-filename" => {
                display_filename =
                    Some(args.next().context("--display-filename requires a value")?);
            }
            "--mime-type" => {
                mime_type = Some(args.next().context("--mime-type requires a value")?);
            }
            "--caption" => {
                caption = Some(args.next().context("--caption requires a value")?);
            }
            "--help" | "-h" => return runtime_wrapper_usage_error(),
            other => bail!("unknown wrapper argument: {other}"),
        }
    }

    let socket_path = socket_from_env_or_arg(socket)?;
    let runtime_rpc_token = runtime_token_from_env_or_arg(token)?;
    match command.as_str() {
        "send" => {
            let target = match (to, group) {
                (Some(recipient), None) => {
                    awiki_deamon::cli_wrapper::OutboundMessageTarget::DirectRecipient(recipient)
                }
                (None, Some(group)) => {
                    awiki_deamon::cli_wrapper::OutboundMessageTarget::Group(group)
                }
                (None, None) => bail!("send requires --to or --group"),
                (Some(_), Some(_)) => {
                    bail!("send accepts either --to or --group, but not both")
                }
            };
            Ok(CliWrapperCommand::Send {
                socket_path,
                runtime_rpc_token,
                target,
                text: text.context("--text is required")?,
                file_path,
                display_filename,
                mime_type,
            })
        }
        "send-message" => Ok(CliWrapperCommand::SendMessage {
            socket_path,
            runtime_rpc_token,
            to_handle: to.context("--to is required")?,
            text: text.context("--text is required")?,
        }),
        "send-attachment" => Ok(CliWrapperCommand::SendAttachment {
            socket_path,
            runtime_rpc_token,
            file_path: file_path.context("--file is required")?,
            display_filename,
            caption,
        }),
        other => bail!("unknown wrapper command: {other}"),
    }
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<DaemonCommand> {
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        return usage_error();
    };

    let mut state_root = None;
    let mut agent_did = None;
    let mut archive_id = None;
    let mut controller_did = None;
    let mut handle = None;
    let mut registration_token = None;
    let mut poll_interval_ms = None;
    let mut max_runtime_ms = None;
    let mut max_processed_messages = None;
    let mut ready_file = None;
    let mut agent_jwt_token = None;
    let mut token = None;
    let mut base_url = None;
    let mut download_base_url = None;
    let mut foreground = false;
    let mut no_service = false;
    let mut print_json = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--agent-did" => {
                let value = args.next().context("--agent-did requires a DID argument")?;
                agent_did = Some(value);
            }
            "--archive-id" => {
                let value = args.next().context("--archive-id requires a value")?;
                archive_id = Some(value);
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
            "--base-url" => {
                let value = args.next().context("--base-url requires a URL argument")?;
                base_url = Some(value);
            }
            "--download-base-url" => {
                let value = args
                    .next()
                    .context("--download-base-url requires a URL argument")?;
                download_base_url = Some(value);
            }
            "--foreground" => {
                foreground = true;
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
            "--no-service" => {
                no_service = true;
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
            "--print-json" => {
                print_json = true;
            }
            "--state-root" => {
                let value = args
                    .next()
                    .context("--state-root requires a path argument")?;
                state_root = Some(PathBuf::from(value));
            }
            "--token" => {
                let value = args.next().context("--token requires a token argument")?;
                token = Some(value);
            }
            "--help" | "-h" => return usage_error(),
            other => bail!("unknown argument: {other}"),
        }
    }

    match command.as_str() {
        "install" => {
            let token = token
                .or_else(|| std::env::var("AWIKI_DAEMON_INSTALL_TOKEN").ok())
                .context("--token or AWIKI_DAEMON_INSTALL_TOKEN is required")?;
            Ok(DaemonCommand::Install {
                options: InstallOptions {
                    token,
                    state_root: state_root_or_default(state_root)?,
                    base_url: base_url.unwrap_or_else(|| "https://awiki.me".to_string()),
                    download_base_url,
                    foreground,
                    no_service,
                    print_json,
                },
            })
        }
        "agent-list" => Ok(DaemonCommand::AgentList {
            state_root: required_state_root(state_root)?,
        }),
        "agent-status" => {
            let state_root = required_state_root(state_root)?;
            let agent_did = agent_did
                .or_else(|| std::env::var("AWIKI_DAEMON_AGENT_DID").ok())
                .context("--agent-did or AWIKI_DAEMON_AGENT_DID is required")?;
            Ok(DaemonCommand::AgentStatus {
                state_root,
                agent_did,
            })
        }
        "foreground" => {
            let state_root = required_state_root(state_root)?;
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
        "init-state" => Ok(DaemonCommand::InitState {
            state_root: required_state_root(state_root)?,
        }),
        "runtime-list" => Ok(DaemonCommand::RuntimeList {
            state_root: required_state_root(state_root)?,
        }),
        "cli-env-capture" => Ok(DaemonCommand::CliEnvCapture {
            state_root: state_root_or_default(state_root)?,
        }),
        "archive-daemon-finalize" => Ok(DaemonCommand::ArchiveDaemonFinalize {
            state_root: required_state_root(state_root)?,
            archive_id: archive_id.context("--archive-id is required")?,
        }),
        "setup-daemon-agent" => {
            let state_root = required_state_root(state_root)?;
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
        "service-status" => Ok(DaemonCommand::Service {
            state_root: state_root_or_default(state_root)?,
            action: ServiceAction::Status,
        }),
        "service-start" => Ok(DaemonCommand::Service {
            state_root: state_root_or_default(state_root)?,
            action: ServiceAction::Start,
        }),
        "service-stop" => Ok(DaemonCommand::Service {
            state_root: state_root_or_default(state_root)?,
            action: ServiceAction::Stop,
        }),
        "service-restart" => Ok(DaemonCommand::Service {
            state_root: state_root_or_default(state_root)?,
            action: ServiceAction::Restart,
        }),
        "service-uninstall" => Ok(DaemonCommand::Service {
            state_root: state_root_or_default(state_root)?,
            action: ServiceAction::Uninstall,
        }),
        "status" => Ok(DaemonCommand::Status {
            state_root: required_state_root(state_root)?,
        }),
        other => bail!("unknown command: {other}"),
    }
}

fn required_state_root(state_root: Option<PathBuf>) -> Result<PathBuf> {
    state_root.context("--state-root is required")
}

fn state_root_or_default(state_root: Option<PathBuf>) -> Result<PathBuf> {
    match state_root {
        Some(state_root) => Ok(state_root),
        None => DaemonConfig::default_product_state_root(),
    }
}

fn usage_error<T>() -> Result<T> {
    bail!("usage: awiki-deamon <install|foreground|init-state|status|service-status|service-start|service-stop|service-restart|service-uninstall|agent-list|agent-status|runtime-list|cli-env-capture|archive-daemon-finalize|setup-daemon-agent> [--state-root <path>] [install: --token <token> --base-url <url> --download-base-url <url> --foreground --no-service --print-json] [--agent-did <did>] [archive-daemon-finalize: --archive-id <id>] [setup-daemon-agent: --handle <handle> --controller-did <did> --registration-token <token>] [foreground: --ready-file <path> --max-runtime-ms <ms> --max-processed-messages <n> --poll-interval-ms <ms> --agent-jwt-token <token>]")
}

fn self_check_usage_error<T>() -> Result<T> {
    bail!("usage: awiki-deamon __self-check [--expected-version <version>]")
}

fn runtime_wrapper_usage_error<T>() -> Result<T> {
    bail!("usage: awiki-deamon-runtime <send|send-message|send-attachment> [--socket <path>] [--token <runtime-token>] [send: (--to <handle-or-did>|--group <group>) --text <text> --file <path> --display-filename <name> --mime-type <mime>] [send-message: --to <handle-or-did> --text <text>] [send-attachment: --file <path> --display-filename <name> --caption <text>]")
}
