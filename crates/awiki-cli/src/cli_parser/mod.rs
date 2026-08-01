use crate::cli_output::ExitError;
use crate::cli_shell::{App, GlobalOptions};
use crate::command_catalog;
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
        self.name == "help" || self.name.starts_with("completion.")
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
        if command_words.is_empty()
            && remaining.is_empty()
            && iter.peek().is_none()
            && matches!(arg.as_str(), "--help" | "-h")
        {
            return Ok(ParsedCommand {
                name: "help".to_string(),
                ..ParsedCommand::default()
            });
        }
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
            Some(("diagnostic", _)) => {
                globals.diagnostic = true;
            }
            Some(("migration", _)) => {
                globals.migration = true;
            }
            Some(("verbose", _)) => {
                globals.verbose = true;
            }
            Some(("internal-service", _)) => {
                globals.internal_service = true;
            }
            Some(("internal-workspace-home", value)) => {
                globals.internal_workspace_home =
                    take_flag_value("internal-workspace-home", value, &mut iter)?;
            }
            Some(("internal-service-user-sid", value)) => {
                globals.internal_service_user_sid =
                    take_flag_value("internal-service-user-sid", value, &mut iter)?;
            }
            Some(("identity", value)) if !is_id_create_context(&command_words) => {
                globals.identity = take_flag_value("identity", value, &mut iter)?;
                globals.identity_changed = true;
            }
            Some(("tenant", value)) => {
                globals.tenant = take_flag_value("tenant", value, &mut iter)?;
                globals.tenant_changed = true;
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

    let remaining = rewrite_help_tail(remaining)?;
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
    enforce_command_policy(command)?;

    match command.name.as_str() {
        "status" => app.run_status(),
        "version" => app.run_version(),
        "upgrade" => app.run_upgrade(),
        "config.show" => app.run_config_show(),
        "tenant.list" => app.run_tenant_list(),
        "tenant.current" => app.run_tenant_current(),
        "tenant.create" => app.run_tenant_create(command),
        "tenant.setup" => app.run_tenant_setup(command),
        "tenant.use" => app.run_tenant_use(command),
        "tenant.reconfigure" => app.run_tenant_reconfigure(command),
        "doctor" => app.run_doctor(),
        "docs" => app.run_docs(&command.args),
        "schema" => app.run_schema(command),
        "help" => app.run_help(command),
        "init" => Err(ExitError::new(
            "unsupported_capability",
            1,
            "sync init is disabled in the async cutover.",
            "Use the async CLI entrypoint.",
        )),
        "onboarding.claim" => app.run_onboarding_claim(command),
        "onboarding.recover-legacy-claim" => Err(async_only_error(&command.name)),
        "onboarding.migrate-legacy" => Err(async_only_error(&command.name)),
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
        "id.vault.status" => app.run_id_vault_status(),
        "id.vault.migrate" => app.run_id_vault_migrate(),
        "id.vault.cleanup-plaintext" => app.run_id_vault_cleanup_plaintext(),
        "id.import-v1" => app.run_id_import_v1(command),
        "id.bind" => app.run_id_bind(command),
        "id.refresh-token" => app.run_id_refresh_token(),
        "id.resolve" => app.run_id_resolve(command),
        "id.replace-did" => app.run_id_replace_did(command),
        "id.profile.get" => app.run_id_profile_get(command),
        "id.profile.set" => app.run_id_profile_set(command),
        "id.device.list"
        | "id.device.join.sessions"
        | "id.device.join.requests"
        | "id.device.join.start"
        | "id.device.join.poll"
        | "id.device.join.verify"
        | "id.device.join.approve"
        | "id.device.join.reject"
        | "id.device.join.cancel"
        | "id.device.revoke" => Err(async_only_error(&command.name)),
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
        "group.secure.status" => app.run_group_secure_status(command),
        "group.secure.repair" => app.run_group_secure_repair(command),
        "group.secure.diagnostics" => app.run_group_secure_diagnostics(),
        "group.e2ee.status" => app.run_group_e2ee_status_alias(command),
        "group.e2ee.publish-key-package" => app.run_group_e2ee_publish_key_package(command),
        "group.e2ee.pending" => app.run_group_e2ee_pending(command),
        "group.e2ee.repair" => app.run_group_e2ee_repair_alias(command),
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
        "debug.db.import-v1" => app.run_debug_db_import_v1(command),
        "debug.db.handle-history" => app.run_debug_db_handle_history(command),
        other if is_go_stub_command(other) => Err(go_stub_error(other)),
        other => Err(stub_error(other)),
    }
}

pub async fn dispatch_async(app: &App, command: &ParsedCommand) -> Result<(), ExitError> {
    enforce_command_policy(command)?;

    match command.name.as_str() {
        "init" => app.run_init_async().await,
        "onboarding.claim" => app.run_onboarding_claim_async(command).await,
        "onboarding.recover-legacy-claim" => {
            app.run_onboarding_recover_legacy_claim_async(command).await
        }
        "onboarding.migrate-legacy" => app.run_onboarding_migrate_legacy_async().await,
        "msg.send" => app.run_msg_send_async(command).await,
        "msg.attachment.download" => app.run_msg_attachment_download_async(command).await,
        "msg.inbox" => app.run_msg_inbox_async(command).await,
        "msg.history" => app.run_msg_history_async(command).await,
        "msg.mark-read" => app.run_msg_mark_read_async(command).await,
        "msg.secure.status" => app.run_msg_secure_status_async(command).await,
        "msg.secure.repair" => app.run_msg_secure_repair_async(command).await,
        "id.register" => app.run_id_register_async(command).await,
        "id.list" => app.run_id_list_async().await,
        "id.current" => app.run_id_current_async().await,
        "id.use" => app.run_id_use_async(command).await,
        "id.status" => app.run_id_status_async().await,
        "id.vault.status" => app.run_id_vault_status_async().await,
        "id.vault.migrate" => app.run_id_vault_migrate_async().await,
        "id.vault.cleanup-plaintext" => app.run_id_vault_cleanup_plaintext_async().await,
        "id.bind" => app.run_id_bind_async(command).await,
        "id.refresh-token" => app.run_id_refresh_token_async().await,
        "id.resolve" => app.run_id_resolve_async(command).await,
        "id.profile.get" => app.run_id_profile_get_async(command).await,
        "id.profile.set" => app.run_id_profile_set_async(command).await,
        "id.device.list" => app.run_id_device_list_async().await,
        "id.device.join.sessions" => app.run_id_device_join_sessions_async().await,
        "id.device.join.requests" => app.run_id_device_join_requests_async().await,
        "id.device.join.start" => app.run_id_device_join_start_async(command).await,
        "id.device.join.poll" => app.run_id_device_join_poll_async(command).await,
        "id.device.join.verify" => app.run_id_device_join_verify_async(command).await,
        "id.device.join.approve" => app.run_id_device_join_approve_async(command).await,
        "id.device.join.reject" => app.run_id_device_join_reject_async(command).await,
        "id.device.join.cancel" => app.run_id_device_join_cancel_async(command).await,
        "id.device.revoke" => app.run_id_device_revoke_async(command).await,
        "id.device.root-key.send" => app.run_id_device_root_key_send_async(command).await,
        "group.create" => app.run_group_create_async(command).await,
        "group.get" => app.run_group_get_async(command).await,
        "group.join" => app.run_group_join_async(command).await,
        "group.add" => app.run_group_add_async(command).await,
        "group.remove" => app.run_group_remove_async(command).await,
        "group.leave" => app.run_group_leave_async(command).await,
        "group.update" => app.run_group_update_async(command).await,
        "group.list" => app.run_group_list_async(command).await,
        "group.members" => app.run_group_members_async(command).await,
        "group.messages" => app.run_group_messages_async(command).await,
        "group.secure.status" => app.run_group_secure_status_async(command).await,
        "group.secure.repair" => app.run_group_secure_repair_async(command).await,
        "group.e2ee.status" => app.run_group_e2ee_status_alias_async(command).await,
        "group.e2ee.publish-key-package" => {
            app.run_group_e2ee_publish_key_package_async(command).await
        }
        "group.e2ee.process-leave-request" => {
            app.run_group_e2ee_process_leave_request_async(command)
                .await
        }
        "group.e2ee.repair" => app.run_group_e2ee_repair_alias_async(command).await,
        "group.e2ee.update-key" => app.run_group_e2ee_update_key_async(command).await,
        "group.e2ee.rejoin" => app.run_group_e2ee_rejoin_async(command).await,
        "group.e2ee.recover-member" => app.run_group_e2ee_recover_member_async(command).await,
        "people.follow" => app.run_people_follow_async(command).await,
        "people.unfollow" => app.run_people_unfollow_async(command).await,
        "people.status" => app.run_people_status_async(command).await,
        "people.followers" => app.run_people_followers_async(command).await,
        "people.following" => app.run_people_following_async(command).await,
        "people.contacts.list" => app.run_people_contacts_list_async(command).await,
        "people.contacts.save" => app.run_people_contacts_save_async(command).await,
        "page.create" => app.run_page_create_async(command).await,
        "page.list" => app.run_page_list_async().await,
        "page.get" => app.run_page_get_async(command).await,
        "page.update" => app.run_page_update_async(command).await,
        "page.rename" => app.run_page_rename_async(command).await,
        "page.delete" => app.run_page_delete_async(command).await,
        "site.root.get" => app.run_site_root_get_async(command).await,
        "site.root.set" => app.run_site_root_set_async(command).await,
        "site.page.list" => app.run_site_page_list_async(command).await,
        "site.page.get" => app.run_site_page_get_async(command).await,
        "site.page.create" => app.run_site_page_create_async(command).await,
        "site.page.update" => app.run_site_page_update_async(command).await,
        "site.page.rename" => app.run_site_page_rename_async(command).await,
        "site.page.delete" => app.run_site_page_delete_async(command).await,
        "runtime.apply" => app.run_runtime_apply_async().await,
        "runtime.setup" => app.run_runtime_setup_async(command).await,
        "runtime.mode.set" => app.run_runtime_mode_set_async(command).await,
        "runtime.listener.run" => app.run_runtime_listener_run_async().await,
        "runtime.listener.service-run" => app.run_runtime_listener_service_run_async().await,
        "mail.inbox" => app.run_mail_inbox_async(command).await,
        "mail.read" => app.run_mail_read_async(command).await,
        "mail.mark-read" => app.run_mail_mark_read_async(command).await,
        "mail.account" => app.run_mail_account_async().await,
        "mail.send" => app.run_mail_send_async(command).await,
        "mail.attachment.download" => app.run_mail_attachment_download_async(command).await,
        "mail.notify" => app.run_mail_notify_async(command).await,
        _ => dispatch(app, command),
    }
}

fn command_name(tokens: &[String]) -> Result<command_catalog::ResolvedCommand, ExitError> {
    if let Some(token) = tokens
        .iter()
        .take_while(|token| !token.starts_with("--"))
        .filter(|token| is_unknown_shorthand_flag(token))
        .next()
    {
        return Err(unknown_shorthand_flag(token));
    }

    let words: Vec<_> = tokens
        .iter()
        .take_while(|token| !token.starts_with("--"))
        .map(String::as_str)
        .collect();
    command_catalog::resolve_command(&words).map_err(|err| match err {
        command_catalog::CommandResolveError::MissingCommand => ExitError::new(
            "invalid_argument",
            2,
            "missing command.",
            "Use `awiki-cli schema` to list command contracts.",
        ),
        command_catalog::CommandResolveError::UnknownSubcommand { parent, subcommand } => {
            unknown_subcommand(parent, &subcommand)
        }
    })
}

fn unknown_subcommand(parent: &str, subcommand: &str) -> ExitError {
    ExitError::new(
        "invalid_argument",
        2,
        format!("unknown command {subcommand:?} for \"awiki-cli {parent}\""),
        format!("Use `awiki-cli {parent} --help` to inspect supported subcommands."),
    )
}

fn rewrite_help_tail(mut tokens: Vec<String>) -> Result<Vec<String>, ExitError> {
    let Some(last) = tokens.last() else {
        return Ok(tokens);
    };
    if !matches!(last.as_str(), "--help" | "-h") {
        return Ok(tokens);
    }
    tokens.pop();
    let words: Vec<_> = tokens
        .iter()
        .take_while(|token| !token.starts_with("--"))
        .map(String::as_str)
        .collect();
    if words.is_empty() {
        return Ok(vec!["help".to_string()]);
    }
    let resolved = command_catalog::resolve_command(&words).map_err(|err| match err {
        command_catalog::CommandResolveError::MissingCommand => ExitError::new(
            "invalid_argument",
            2,
            "missing command.",
            "Use `awiki-cli schema` to list command contracts.",
        ),
        command_catalog::CommandResolveError::UnknownSubcommand { parent, subcommand } => {
            unknown_subcommand(parent, &subcommand)
        }
    })?;
    let spec = command_catalog::lookup(&resolved.name).ok_or_else(|| {
        ExitError::new(
            "not_found",
            5,
            format!("Unknown command help target {:?}", words.join(" ")),
            "Use `awiki-cli schema` to list command contracts.",
        )
    })?;
    Ok(["help".to_string()]
        .into_iter()
        .chain(spec.name.split('.').map(str::to_string))
        .collect())
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
                None if command_catalog::is_local_bool_flag(command_name, name) => {
                    "true".to_string()
                }
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
    if command_catalog::lookup(command_name).is_none()
        || command_catalog::has_local_flag(command_name, flag_name)
    {
        return Ok(());
    }
    Err(unknown_long_flag(flag_name))
}

fn unknown_long_flag(flag_name: &str) -> ExitError {
    ExitError::new(
        "invalid_argument",
        2,
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
        "invalid_argument",
        2,
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
    command_catalog::lookup(command).is_some_and(|spec| spec.handler == "stub")
}

fn enforce_command_policy(command: &ParsedCommand) -> Result<(), ExitError> {
    match command_catalog::direct_invocation_policy(&command.name) {
        command_catalog::DirectInvocationPolicy::Allow => Ok(()),
        command_catalog::DirectInvocationPolicy::AllowWithWarning => Ok(()),
        command_catalog::DirectInvocationPolicy::RequireDiagnosticGate => {
            if command.globals.diagnostic
                || std::env::var("AWIKI_CLI_ENABLE_DIAGNOSTIC").ok().as_deref() == Some("1")
            {
                Ok(())
            } else {
                Err(diagnostic_gate_required(&command.name))
            }
        }
        command_catalog::DirectInvocationPolicy::RequireMigrationGate => {
            if command.globals.migration
                || std::env::var("AWIKI_CLI_ENABLE_MIGRATION").ok().as_deref() == Some("1")
            {
                Ok(())
            } else {
                Err(migration_gate_required(&command.name))
            }
        }
        command_catalog::DirectInvocationPolicy::RequireInternalServiceGate => {
            if command.globals.internal_service
                || std::env::var("AWIKI_CLI_INTERNAL_ENTRY").ok().as_deref() == Some("1")
            {
                Ok(())
            } else {
                Err(internal_command(&command.name))
            }
        }
        command_catalog::DirectInvocationPolicy::StableUnsupported { capability, phase } => {
            Err(crate::cli_shell::unsupported::unsupported_cutover_command(
                &command.name,
                capability,
                phase,
            ))
        }
        command_catalog::DirectInvocationPolicy::Removed { replacement } => {
            Err(removed_command(&command.name, replacement))
        }
        command_catalog::DirectInvocationPolicy::DeprecatedAlias { .. } => Ok(()),
    }
}

fn diagnostic_gate_required(command: &str) -> ExitError {
    let mut err = ExitError::new(
        "diagnostic_gate_required",
        2,
        format!("{command} is a diagnostic command."),
        "Re-run with --diagnostic or inspect schema --audience diagnostic.",
    );
    err.detail.details = serde_json::json!({
        "command": command,
        "audience": "diagnostic",
        "required_gate": "--diagnostic",
    });
    err
}

fn migration_gate_required(command: &str) -> ExitError {
    let mut err = ExitError::new(
        "migration_gate_required",
        2,
        format!("{command} is a migration-only command."),
        "Re-run with --migration or inspect schema --audience migration.",
    );
    err.detail.details = serde_json::json!({
        "command": command,
        "audience": "migration",
        "required_gate": "--migration",
    });
    err
}

fn internal_command(command: &str) -> ExitError {
    let mut err = ExitError::new(
        "internal_command",
        2,
        format!("{command} is an internal service entry."),
        "Use the high-level runtime command, or let the service manager launch this entry.",
    );
    err.detail.details = serde_json::json!({
        "command": command,
        "audience": "internal",
        "required_gate": "AWIKI_CLI_INTERNAL_ENTRY=1",
    });
    err
}

fn removed_command(command: &str, replacement: Option<&str>) -> ExitError {
    let mut err = ExitError::new(
        "removed_command",
        2,
        format!("{command} is removed from the im-core CLI cutover path."),
        replacement
            .map(|value| format!("Use `{value}` instead."))
            .unwrap_or_else(|| {
                "Use high-level im-core commands instead of raw/internal commands.".to_string()
            }),
    );
    err.detail.details = serde_json::json!({
        "command": command,
        "replacement": replacement,
    });
    err
}

fn go_stub_error(command: &str) -> ExitError {
    if let command_catalog::CutoverStatus::Unsupported { capability, phase } =
        command_catalog::cutover_status(command)
    {
        return crate::cli_shell::unsupported::unsupported_cutover_command(
            command, capability, phase,
        );
    }

    let spec = command_catalog::lookup(command).expect("known Go stub command");
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
    let target = command_catalog::lookup(command)
        .map(|spec| spec.phase)
        .unwrap_or("phase1");
    ExitError::new(
        "not_implemented",
        1,
        format!("{command} is not implemented in this Rust port slice."),
        format!("Use `awiki-cli schema {command}` to inspect the {target} contract."),
    )
}

fn async_only_error(command: &str) -> ExitError {
    ExitError::new(
        "unsupported_capability",
        1,
        format!("sync {command} is disabled in the async cutover."),
        "Use the async CLI entrypoint.",
    )
}
