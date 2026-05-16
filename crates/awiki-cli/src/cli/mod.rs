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
        ["upgrade", ..] => "upgrade",
        ["doctor", ..] => "doctor",
        ["docs", ..] => "docs",
        ["schema", ..] => "schema",
        ["init", ..] => "init",
        ["config", "show", ..] => "config.show",
        ["config", "set", ..] => "config.set",
        ["completion", "bash", ..] => "completion.bash",
        ["completion", "zsh", ..] => "completion.zsh",
        ["completion", "fish", ..] => "completion.fish",
        ["completion", "powershell", ..] => "completion.powershell",
        ["id", "create", ..] => "id.create",
        ["id", "register", ..] => "id.register",
        ["id", "list", ..] => "id.list",
        ["id", "current", ..] => "id.current",
        ["id", "use", ..] => "id.use",
        ["id", "status", ..] => "id.status",
        ["id", "import-v1", ..] => "id.import-v1",
        ["id", "bind", ..] => "id.bind",
        ["id", "refresh-token", ..] => "id.refresh-token",
        ["id", "resolve", ..] => "id.resolve",
        ["id", "recover", ..] => "id.recover",
        ["id", "replace-did", ..] => "id.replace-did",
        ["id", "profile", "get", ..] => "id.profile.get",
        ["id", "profile", "set", ..] => "id.profile.set",
        ["msg", "send", ..] => "msg.send",
        ["msg", "attachment", "download", ..] => "msg.attachment.download",
        ["msg", "inbox", ..] => "msg.inbox",
        ["msg", "history", ..] => "msg.history",
        ["msg", "mark-read", ..] => "msg.mark-read",
        ["msg", "secure", "status", ..] => "msg.secure.status",
        ["msg", "secure", "init", ..] => "msg.secure.init",
        ["msg", "secure", "repair", ..] => "msg.secure.repair",
        ["msg", "secure", "failed", ..] => "msg.secure.failed",
        ["msg", "secure", "retry", ..] => "msg.secure.retry",
        ["msg", "secure", "drop", ..] => "msg.secure.drop",
        ["mail", "inbox", ..] => "mail.inbox",
        ["mail", "read", ..] => "mail.read",
        ["mail", "mark-read", ..] => "mail.mark-read",
        ["mail", "account", ..] => "mail.account",
        ["mail", "send", ..] => "mail.send",
        ["mail", "attachment", "download", ..] => "mail.attachment.download",
        ["mail", "notify", ..] => "mail.notify",
        ["group", "create", ..] => "group.create",
        ["group", "get", ..] | ["group", "show", ..] => "group.get",
        ["group", "join", ..] => "group.join",
        ["group", "add", ..] => "group.add",
        ["group", "remove", ..] | ["group", "kick", ..] => "group.remove",
        ["group", "leave", ..] => "group.leave",
        ["group", "update", ..] => "group.update",
        ["group", "list", ..] => "group.list",
        ["group", "members", ..] => "group.members",
        ["group", "messages", ..] => "group.messages",
        ["group", "e2ee", "status", ..] => "group.e2ee.status",
        ["group", "e2ee", "publish-key-package", ..] => "group.e2ee.publish-key-package",
        ["group", "e2ee", "pending", ..] => "group.e2ee.pending",
        ["group", "e2ee", "repair", ..] => "group.e2ee.repair",
        ["group", "e2ee", "update-key", ..] => "group.e2ee.update-key",
        ["group", "e2ee", "rejoin", ..] => "group.e2ee.rejoin",
        ["group", "e2ee", "recover-member", ..] => "group.e2ee.recover-member",
        ["group", "e2ee", "process-leave-request", ..] => "group.e2ee.process-leave-request",
        ["page", "create", ..] => "page.create",
        ["page", "list", ..] => "page.list",
        ["page", "get", ..] => "page.get",
        ["page", "update", ..] => "page.update",
        ["page", "rename", ..] => "page.rename",
        ["page", "delete", ..] => "page.delete",
        ["site", "root", "get", ..] => "site.root.get",
        ["site", "root", "set", ..] => "site.root.set",
        ["site", "page", "list", ..] => "site.page.list",
        ["site", "page", "get", ..] => "site.page.get",
        ["site", "page", "create", ..] => "site.page.create",
        ["site", "page", "update", ..] => "site.page.update",
        ["site", "page", "rename", ..] => "site.page.rename",
        ["site", "page", "delete", ..] => "site.page.delete",
        ["runtime", "status", ..] => "runtime.status",
        ["runtime", "apply", ..] => "runtime.apply",
        ["runtime", "setup", ..] => "runtime.setup",
        ["runtime", "mode", "get", ..] => "runtime.mode.get",
        ["runtime", "mode", "set", ..] => "runtime.mode.set",
        ["runtime", "listener", "status", ..] => "runtime.listener.status",
        ["runtime", "listener", "install", ..] => "runtime.listener.install",
        ["runtime", "listener", "start", ..] => "runtime.listener.start",
        ["runtime", "listener", "stop", ..] => "runtime.listener.stop",
        ["runtime", "listener", "restart", ..] => "runtime.listener.restart",
        ["runtime", "listener", "uninstall", ..] => "runtime.listener.uninstall",
        ["runtime", "listener", "config", "show", ..] => "runtime.listener.config.show",
        ["runtime", "listener", "config", "set", ..] => "runtime.listener.config.set",
        ["runtime", "listener", "enable", ..] => "runtime.listener.enable",
        ["runtime", "listener", "disable", ..] => "runtime.listener.disable",
        ["runtime", "listener", "run", ..] => "runtime.listener.run",
        ["runtime", "listener", "service-run", ..] => "runtime.listener.service-run",
        ["runtime", "host-notify", "enable", ..] => "runtime.host-notify.enable",
        ["runtime", "host-notify", "disable", ..] => "runtime.host-notify.disable",
        ["runtime", "host-notify", "config", "show", ..] => "runtime.host-notify.config.show",
        ["runtime", "host-notify", "config", "set", ..] => "runtime.host-notify.config.set",
        ["runtime", "host-notify", "openclaw", "set", ..] => "runtime.host-notify.openclaw.set",
        ["runtime", "host-notify", "openclaw", "set-token", ..] => {
            "runtime.host-notify.openclaw.set-token"
        }
        ["runtime", "host-notify", "openclaw", "clear-token", ..] => {
            "runtime.host-notify.openclaw.clear-token"
        }
        ["runtime", "host-notify", "openclaw", "route", "add", ..] => {
            "runtime.host-notify.openclaw.route.add"
        }
        ["runtime", "host-notify", "openclaw", "route", "list", ..] => {
            "runtime.host-notify.openclaw.route.list"
        }
        ["runtime", "host-notify", "openclaw", "route", "remove", ..] => {
            "runtime.host-notify.openclaw.route.remove"
        }
        ["runtime", "host-notify", "hermes" | "webhook", "guide", ..] => {
            "runtime.host-notify.hermes.guide"
        }
        ["runtime", "host-notify", "hermes" | "webhook", "status", ..] => {
            "runtime.host-notify.hermes.status"
        }
        ["runtime", "host-notify", "hermes" | "webhook", "setup", ..] => {
            "runtime.host-notify.hermes.setup"
        }
        ["runtime", "host-notify", "hermes" | "webhook", "set", ..] => {
            "runtime.host-notify.hermes.set"
        }
        ["runtime", "host-notify", "hermes" | "webhook", "set-secret", ..] => {
            "runtime.host-notify.hermes.set-secret"
        }
        ["runtime", "host-notify", "hermes" | "webhook", "clear-secret", ..] => {
            "runtime.host-notify.hermes.clear-secret"
        }
        ["runtime", "host-notify", "hermes", "bridge", "service-run", ..] => {
            "runtime.host-notify.hermes.bridge.service-run"
        }
        ["debug", "db", "query", ..] => "debug.db.query",
        ["debug", "db", "import-v1", ..] => "debug.db.import-v1",
        ["debug", "db", "handle-history", ..] => "debug.db.handle-history",
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
        "enabled"
            | "auto-install"
            | "auto-start"
            | "all"
            | "wait"
            | "self"
            | "unread"
            | "mark-read"
            | "is-public"
            | "is-agent"
            | "e2ee"
            | "recovery"
            | "contract-test"
            | "attachments-allowed"
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
