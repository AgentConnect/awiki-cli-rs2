use crate::app::{App, GlobalOptions};
use crate::cmdmeta;
use crate::output::ExitError;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub struct ParsedCommand {
    pub globals: GlobalOptions,
    pub name: String,
    pub args: Vec<String>,
    pub flags: BTreeMap<String, String>,
    pub changed_flags: Vec<String>,
}

pub fn parse_env() -> Result<ParsedCommand, ExitError> {
    parse_args(std::env::args().skip(1))
}

fn parse_args<I>(args: I) -> Result<ParsedCommand, ExitError>
where
    I: IntoIterator<Item = String>,
{
    let mut globals = GlobalOptions::default();
    let mut remaining = Vec::new();
    let mut command_words = Vec::new();
    let mut iter = args.into_iter().peekable();
    while let Some(arg) = iter.next() {
        match split_long_flag(&arg) {
            Some(("format", value)) => {
                globals.format = take_flag_value("format", value, &mut iter)?;
                globals.format_changed = true;
            }
            Some(("jq", value)) => {
                globals.jq = take_flag_value("jq", value, &mut iter)?;
            }
            Some(("dry-run", _)) => {
                globals.dry_run = true;
            }
            Some(("verbose", _)) => {
                globals.verbose = true;
            }
            Some(("identity", value)) if !is_id_create_context(&command_words) => {
                globals.identity = take_flag_value("identity", value, &mut iter)?;
                globals.identity_changed = true;
            }
            _ => {
                if !arg.starts_with("--") {
                    command_words.push(arg.clone());
                }
                remaining.push(arg);
            }
        }
    }

    let name = command_name(&remaining)?;
    let path_len = name.split('.').filter(|part| !part.is_empty()).count();
    let tail = drop_command_words(&remaining, path_len);
    let (flags, changed_flags, args) = parse_local_tail(&tail)?;
    Ok(ParsedCommand {
        globals,
        name,
        args,
        flags,
        changed_flags,
    })
}

pub fn dispatch(app: &App, command: &ParsedCommand) -> Result<(), ExitError> {
    match command.name.as_str() {
        "status" => app.run_status(),
        "version" => app.run_version(),
        "config.show" => app.run_config_show(),
        "doctor" => app.run_doctor(),
        "docs" => app.run_docs(&command.args),
        "schema" => app.run_schema(&command.args),
        "init" => app.run_init(),
        "completion.bash" => app.run_completion("bash"),
        "completion.zsh" => app.run_completion("zsh"),
        "completion.fish" => app.run_completion("fish"),
        "completion.powershell" => app.run_completion("powershell"),
        "id.create" => app.run_id_create(command),
        "id.refresh-token" => app.run_id_refresh_token(),
        "msg.send" => app.run_msg_send(command),
        "page.create" => app.run_page_create(command),
        "runtime.status" => app.run_runtime_status(),
        "runtime.setup" => app.run_runtime_setup(command),
        "runtime.listener.config.set" => app.run_runtime_listener_config_set(command),
        "debug.db.query" => app.run_debug_db_query(command),
        "debug.db.import-v1" => app.run_debug_db_import_v1(command),
        other => Err(stub_error(other)),
    }
}

fn command_name(tokens: &[String]) -> Result<String, ExitError> {
    let words: Vec<_> = tokens
        .iter()
        .take_while(|token| !token.starts_with("--"))
        .map(String::as_str)
        .collect();
    let name = match words.as_slice() {
        [] => "",
        ["status", ..] => "status",
        ["version", ..] => "version",
        ["doctor", ..] => "doctor",
        ["docs", ..] => "docs",
        ["schema", ..] => "schema",
        ["init", ..] => "init",
        ["config", "show", ..] => "config.show",
        ["completion", "bash", ..] => "completion.bash",
        ["completion", "zsh", ..] => "completion.zsh",
        ["completion", "fish", ..] => "completion.fish",
        ["completion", "powershell", ..] => "completion.powershell",
        ["id", "create", ..] => "id.create",
        ["id", "refresh-token", ..] => "id.refresh-token",
        ["msg", "send", ..] => "msg.send",
        ["page", "create", ..] => "page.create",
        ["runtime", "status", ..] => "runtime.status",
        ["runtime", "setup", ..] => "runtime.setup",
        ["runtime", "listener", "config", "set", ..] => "runtime.listener.config.set",
        ["debug", "db", "query", ..] => "debug.db.query",
        ["debug", "db", "import-v1", ..] => "debug.db.import-v1",
        [head, ..] => head,
    };
    if name.is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "missing command.",
            "Use `awiki-cli schema` to list command contracts.",
        ));
    }
    Ok(name.to_string())
}

fn drop_command_words(tokens: &[String], path_len: usize) -> Vec<String> {
    let mut remaining_path = path_len;
    let mut tail = Vec::new();
    for token in tokens {
        if remaining_path > 0 && !token.starts_with("--") {
            remaining_path -= 1;
            continue;
        }
        tail.push(token.clone());
    }
    tail
}

fn parse_local_tail(
    tokens: &[String],
) -> Result<(BTreeMap<String, String>, Vec<String>, Vec<String>), ExitError> {
    let mut flags = BTreeMap::new();
    let mut changed_flags = Vec::new();
    let mut args = Vec::new();
    let mut iter = tokens.iter().peekable();
    while let Some(token) = iter.next() {
        if let Some((name, value)) = split_long_flag(token) {
            let value = match value {
                Some(value) => value.to_string(),
                None if is_bool_local_flag(name) => "true".to_string(),
                None => iter.next().map(|value| value.to_string()).ok_or_else(|| {
                    ExitError::new(
                        "invalid_argument",
                        2,
                        format!("--{name} requires a value."),
                        "Pass a value after the flag.",
                    )
                })?,
            };
            flags.insert(name.to_string(), value);
            changed_flags.push(name.to_string());
        } else {
            args.push(token.to_string());
        }
    }
    Ok((flags, changed_flags, args))
}

fn split_long_flag(arg: &str) -> Option<(&str, Option<&str>)> {
    let body = arg.strip_prefix("--")?;
    if let Some((name, value)) = body.split_once('=') {
        return Some((name, Some(value)));
    }
    Some((body, None))
}

fn take_flag_value<I>(
    name: &str,
    inline: Option<&str>,
    iter: &mut std::iter::Peekable<I>,
) -> Result<String, ExitError>
where
    I: Iterator<Item = String>,
{
    if let Some(value) = inline {
        return Ok(value.to_string());
    }
    iter.next().ok_or_else(|| {
        ExitError::new(
            "invalid_argument",
            2,
            format!("--{name} requires a value."),
            "Pass a value after the flag.",
        )
    })
}

fn is_id_create_context(words: &[String]) -> bool {
    words.len() >= 2 && words[0] == "id" && words[1] == "create"
}

fn is_bool_local_flag(name: &str) -> bool {
    matches!(
        name,
        "enabled" | "auto-install" | "auto-start" | "all" | "wait" | "secure"
    )
}

fn stub_error(command: &str) -> ExitError {
    let target = cmdmeta::lookup(command)
        .map(|spec| spec.phase)
        .unwrap_or("phase1");
    ExitError::new(
        "not_implemented",
        1,
        format!("{command} is not implemented in this Rust port slice."),
        format!("Use `awiki-cli schema {command}` to inspect the {target} contract."),
    )
}
