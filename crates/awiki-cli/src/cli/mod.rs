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

impl ParsedCommand {
    pub fn trace_command(&self) -> String {
        let name = self.name.trim();
        if name.is_empty() {
            "awiki-cli".to_string()
        } else {
            format!("awiki-cli {}", name.replace('.', " "))
        }
    }

    pub fn emits_raw_output(&self) -> bool {
        self.name.starts_with("completion.")
    }
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
            Some((name, _)) if command_words.is_empty() && !name.is_empty() => {
                return Err(unknown_long_flag(name));
            }
            _ => {
                if !arg.starts_with("--") {
                    command_words.push(arg.clone());
                }
                remaining.push(arg);
            }
        }
    }

    let resolved = command_name(&remaining)?;
    let name = resolved.name;
    let path_len = resolved.consumed_words;
    let tail = drop_command_words(&remaining, path_len);
    let (flags, changed_flags, args) = parse_local_tail(&name, &tail)?;
    Ok(ParsedCommand {
        globals,
        name,
        args,
        flags,
        changed_flags,
    })
}

pub fn dispatch(app: &App, command: &ParsedCommand) -> Result<(), ExitError> {
    if let Some(err) = default_cutover_boundary_error(&command.name) {
        return Err(err);
    }

    match command.name.as_str() {
        "status" => app.run_status(),
        "version" => app.run_version(),
        "upgrade" => app.run_upgrade(),
        "config.show" => app.run_config_show(),
        "config.set" => app.run_config_set(command),
        "doctor" => app.run_doctor(),
        "docs" => app.run_docs(&command.args),
        "schema" => app.run_schema(&command.args),
        "init" => app.run_init(),
        "completion.bash" => app.run_completion("bash"),
        "completion.zsh" => app.run_completion("zsh"),
        "completion.fish" => app.run_completion("fish"),
        "completion.powershell" => app.run_completion("powershell"),
        "id.create" => app.run_id_create(command),
        "id.register" => app.run_id_register(command),
        "id.list" => app.run_id_list(),
        "id.current" => app.run_id_current(),
        "id.use" => app.run_id_use(command),
        "id.status" => app.run_id_status(),
        "id.import-v1" => app.run_id_import_v1(command),
        "id.bind" => app.run_id_bind(command),
        "id.refresh-token" => app.run_id_refresh_token(),
        "id.resolve" => app.run_id_resolve(command),
        "id.recover" => app.run_id_recover(command),
        "id.replace-did" => app.run_id_replace_did(command),
        "id.profile.get" => app.run_id_profile_get(command),
        "id.profile.set" => app.run_id_profile_set(command),
        "msg.send" => app.run_msg_send(command),
        "msg.attachment.download" => app.run_msg_attachment_download(command),
        "msg.inbox" => app.run_msg_inbox(command),
        "msg.history" => app.run_msg_history(command),
        "msg.mark-read" => app.run_msg_mark_read(command),
        "msg.secure.status" => app.run_msg_secure_status(command),
        "msg.secure.init" => app.run_msg_secure_init(command),
        "msg.secure.repair" => app.run_msg_secure_repair(command),
        "msg.secure.failed" => app.run_msg_secure_failed(),
        "msg.secure.retry" => app.run_msg_secure_retry(command),
        "msg.secure.drop" => app.run_msg_secure_drop(command),
        "mail.inbox" => app.run_mail_inbox(command),
        "mail.read" => app.run_mail_read(command),
        "mail.mark-read" => app.run_mail_mark_read(command),
        "mail.account" => app.run_mail_account(),
        "mail.send" => app.run_mail_send(command),
        "mail.attachment.download" => app.run_mail_attachment_download(command),
        "mail.notify" => app.run_mail_notify(command),
        "group.create" => app.run_group_create(command),
        "group.get" => app.run_group_get(command),
        "group.join" => app.run_group_join(command),
        "group.add" => app.run_group_add(command),
        "group.remove" => app.run_group_remove(command),
        "group.leave" => app.run_group_leave(command),
        "group.update" => app.run_group_update(command),
        "group.list" => app.run_group_list(command),
        "group.members" => app.run_group_members(command),
        "group.messages" => app.run_group_messages(command),
        "group.e2ee.status" => app.run_group_e2ee_status(command),
        "group.e2ee.publish-key-package" => app.run_group_e2ee_publish_key_package(command),
        "group.e2ee.pending" => app.run_group_e2ee_pending(command),
        "group.e2ee.repair" => app.run_group_e2ee_repair(command),
        "group.e2ee.update-key" => app.run_group_e2ee_update_key(command),
        "group.e2ee.rejoin" => app.run_group_e2ee_rejoin(command),
        "group.e2ee.recover-member" => app.run_group_e2ee_recover_member(command),
        "group.e2ee.process-leave-request" => app.run_group_e2ee_process_leave_request(command),
        "people.follow" => app.run_people_follow(command),
        "people.unfollow" => app.run_people_unfollow(command),
        "people.status" => app.run_people_status(command),
        "people.followers" => app.run_people_followers(command),
        "people.following" => app.run_people_following(command),
        "people.contacts.list" => app.run_people_contacts_list(command),
        "people.contacts.save" => app.run_people_contacts_save(command),
        "page.create" => app.run_page_create(command),
        "page.list" => app.run_page_list(),
        "page.get" => app.run_page_get(command),
        "page.update" => app.run_page_update(command),
        "page.rename" => app.run_page_rename(command),
        "page.delete" => app.run_page_delete(command),
        "site.root.get" => app.run_site_root_get(command),
        "site.root.set" => app.run_site_root_set(command),
        "site.page.list" => app.run_site_page_list(command),
        "site.page.get" => app.run_site_page_get(command),
        "site.page.create" => app.run_site_page_create(command),
        "site.page.update" => app.run_site_page_update(command),
        "site.page.rename" => app.run_site_page_rename(command),
        "site.page.delete" => app.run_site_page_delete(command),
        "runtime.status" => app.run_runtime_status(),
        "runtime.apply" => app.run_runtime_apply(),
        "runtime.setup" => app.run_runtime_setup(command),
        "runtime.mode.get" => app.run_runtime_mode_get(),
        "runtime.mode.set" => app.run_runtime_mode_set(command),
        "runtime.listener.status" => app.run_runtime_listener_status(),
        "runtime.listener.install" => app.run_runtime_listener_install(),
        "runtime.listener.start" => app.run_runtime_listener_start(),
        "runtime.listener.stop" => app.run_runtime_listener_stop(),
        "runtime.listener.restart" => app.run_runtime_listener_restart(),
        "runtime.listener.uninstall" => app.run_runtime_listener_uninstall(),
        "runtime.listener.run" => app.run_runtime_listener_run(),
        "runtime.listener.service-run" => app.run_runtime_listener_service_run(),
        "runtime.listener.config.show" => app.run_runtime_listener_config_show(),
        "runtime.listener.config.set" => app.run_runtime_listener_config_set(command),
        "runtime.listener.enable" => app.run_runtime_listener_enable(),
        "runtime.listener.disable" => app.run_runtime_listener_disable(),
        "runtime.host-notify.enable" => app.run_runtime_host_notify_enable(),
        "runtime.host-notify.disable" => app.run_runtime_host_notify_disable(),
        "runtime.host-notify.config.show" => app.run_runtime_host_notify_config_show(),
        "runtime.host-notify.config.set" => app.run_runtime_host_notify_config_set(command),
        "runtime.host-notify.openclaw.set" => app.run_runtime_host_notify_openclaw_set(command),
        "runtime.host-notify.openclaw.set-token" => {
            app.run_runtime_host_notify_openclaw_set_token(command)
        }
        "runtime.host-notify.openclaw.clear-token" => {
            app.run_runtime_host_notify_openclaw_clear_token()
        }
        "runtime.host-notify.openclaw.route.add" => {
            app.run_runtime_host_notify_openclaw_route_add(command)
        }
        "runtime.host-notify.openclaw.route.list" => {
            app.run_runtime_host_notify_openclaw_route_list()
        }
        "runtime.host-notify.openclaw.route.remove" => {
            app.run_runtime_host_notify_openclaw_route_remove(command)
        }
        "runtime.host-notify.hermes.guide" => app.run_runtime_host_notify_hermes_guide(command),
        "runtime.host-notify.hermes.status" => app.run_runtime_host_notify_hermes_status(),
        "runtime.host-notify.hermes.setup" => app.run_runtime_host_notify_hermes_setup(command),
        "runtime.host-notify.hermes.bridge.service-run" => {
            app.run_runtime_host_notify_hermes_bridge_service_run()
        }
        "runtime.host-notify.hermes.set" => app.run_runtime_host_notify_hermes_set(command),
        "runtime.host-notify.hermes.set-secret" => {
            app.run_runtime_host_notify_hermes_set_secret(command)
        }
        "runtime.host-notify.hermes.clear-secret" => {
            app.run_runtime_host_notify_hermes_clear_secret()
        }
        "debug.db.query" => app.run_debug_db_query(command),
        "debug.db.import-v1" => app.run_debug_db_import_v1(command),
        "debug.db.handle-history" => app.run_debug_db_handle_history(command),
        other if is_go_stub_command(other) => Err(go_stub_error(other)),
        other => Err(stub_error(other)),
    }
}

fn command_name(tokens: &[String]) -> Result<cmdmeta::ResolvedCommand, ExitError> {
    for token in tokens
        .iter()
        .take_while(|token| !token.starts_with("--"))
        .filter(|token| is_unknown_shorthand_flag(token))
    {
        return Err(unknown_shorthand_flag(token));
    }

    let words: Vec<_> = tokens
        .iter()
        .take_while(|token| !token.starts_with("--"))
        .map(String::as_str)
        .collect();
    cmdmeta::resolve_command(&words).map_err(|err| match err {
        cmdmeta::CommandResolveError::MissingCommand => ExitError::new(
            "invalid_argument",
            2,
            "missing command.",
            "Use `awiki-cli schema` to list command contracts.",
        ),
        cmdmeta::CommandResolveError::UnknownSubcommand { parent, subcommand } => {
            unknown_subcommand(parent, &subcommand)
        }
    })
}

fn unknown_subcommand(parent: &str, subcommand: &str) -> ExitError {
    ExitError::new(
        "internal_error",
        1,
        format!("unknown command {subcommand:?} for \"awiki-cli {parent}\""),
        format!("Use `awiki-cli {parent} --help` to inspect supported subcommands."),
    )
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
    command_name: &str,
    tokens: &[String],
) -> Result<(BTreeMap<String, String>, Vec<String>, Vec<String>), ExitError> {
    let mut flags = BTreeMap::new();
    let mut changed_flags = Vec::new();
    let mut args = Vec::new();
    let mut iter = tokens.iter().peekable();
    while let Some(token) = iter.next() {
        if let Some((name, value)) = split_long_flag(token) {
            validate_local_flag(command_name, name)?;
            let value = match value {
                Some(value) => value.to_string(),
                None if cmdmeta::is_local_bool_flag(command_name, name) => "true".to_string(),
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
        } else if is_unknown_shorthand_flag(token) {
            return Err(unknown_shorthand_flag(token));
        } else {
            args.push(token.to_string());
        }
    }
    Ok((flags, changed_flags, args))
}

fn validate_local_flag(command_name: &str, flag_name: &str) -> Result<(), ExitError> {
    if cmdmeta::lookup(command_name).is_none() || cmdmeta::has_local_flag(command_name, flag_name) {
        return Ok(());
    }
    Err(unknown_long_flag(flag_name))
}

fn unknown_long_flag(flag_name: &str) -> ExitError {
    ExitError::new(
        "internal_error",
        1,
        format!("unknown flag: --{flag_name}"),
        "",
    )
}

fn split_long_flag(arg: &str) -> Option<(&str, Option<&str>)> {
    let body = arg.strip_prefix("--")?;
    if let Some((name, value)) = body.split_once('=') {
        return Some((name, Some(value)));
    }
    Some((body, None))
}

fn is_unknown_shorthand_flag(token: &str) -> bool {
    token.starts_with('-') && !token.starts_with("--") && token.len() > 1 && token != "-h"
}

fn unknown_shorthand_flag(token: &str) -> ExitError {
    let shorthand = token
        .trim_start_matches('-')
        .chars()
        .next()
        .unwrap_or_default();
    ExitError::new(
        "internal_error",
        1,
        format!("unknown shorthand flag: '{shorthand}' in {token}"),
        "",
    )
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

fn is_go_stub_command(command: &str) -> bool {
    cmdmeta::lookup(command).is_some_and(|spec| spec.handler == "stub")
}

fn default_cutover_boundary_error(command: &str) -> Option<ExitError> {
    if !is_default_cutover_blocked_domain(command) {
        return None;
    }
    match cmdmeta::cutover_status(command) {
        cmdmeta::CutoverStatus::Unsupported { capability, phase } => Some(
            crate::app::unsupported::unsupported_cutover_command(command, capability, phase),
        ),
        cmdmeta::CutoverStatus::Removed if command == "debug.raw.rpc" => {
            Some(crate::app::unsupported::unsupported_cutover_command(
                command,
                "raw-rpc",
                "outside current im-core cutover",
            ))
        }
        _ => None,
    }
}

fn is_default_cutover_blocked_domain(command: &str) -> bool {
    command == "debug.db.query" || command == "debug.raw.rpc"
}

fn go_stub_error(command: &str) -> ExitError {
    if let cmdmeta::CutoverStatus::Unsupported { capability, phase } =
        cmdmeta::cutover_status(command)
    {
        return crate::app::unsupported::unsupported_cutover_command(command, capability, phase);
    }

    let spec = cmdmeta::lookup(command).expect("known Go stub command");
    let command_path = format!("awiki-cli {}", command.replace('.', " "));
    ExitError::new(
        "internal_error",
        1,
        format!("{command_path} is not implemented yet."),
        format!(
            "{command_path} is planned for {}. Use `awiki-cli schema {command}` to inspect the frozen contract.",
            spec.phase.to_ascii_uppercase()
        ),
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
