use super::{identity_exit, App};
use crate::cli::ParsedCommand;
use crate::output::ExitError;
use crate::site::{self, CommandResult, SiteError};
use serde_json::json;
use std::fs;

impl App {
    pub fn run_site_root_get(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        require_flags(command, &["domain"])?;
        let resolved = self.resolve_config_for_workspace()?;
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli site root get",
                &resolved,
                json!({
                    "plan": {
                        "action": "site.root.get",
                        "identity": self.globals.identity,
                        "rpc_endpoint": "/site/rpc",
                        "rpc_method": "get_root",
                        "request": {
                            "domain": string_flag(command, "domain").trim(),
                        },
                    }
                }),
                "Dry run: site root get planned",
                Vec::new(),
            );
        }
        let result = site::get_root(
            &resolved,
            &self.identity_manager(&resolved),
            &string_flag(command, "domain"),
        )
        .map_err(|err| {
            site_exit(
                err,
                "Make sure the active identity is a configured tenant site admin for the requested domain.",
            )
        })?;
        self.render_site_result("awiki-cli site root get", &resolved, result)
    }

    pub fn run_site_root_set(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        require_flags(command, &["domain"])?;
        let body = required_markdown_body(command)?;
        let resolved = self.resolve_config_for_workspace()?;
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli site root set",
                &resolved,
                json!({
                    "plan": {
                        "action": "site.root.set",
                        "identity": self.globals.identity,
                        "rpc_endpoint": "/site/rpc",
                        "rpc_method": "set_root",
                        "request": {
                            "domain": string_flag(command, "domain").trim(),
                            "body_bytes": body.len(),
                        },
                    }
                }),
                "Dry run: site root set planned",
                Vec::new(),
            );
        }
        let result = site::set_root(
            &resolved,
            &self.identity_manager(&resolved),
            site::SetRootParams {
                domain: string_flag(command, "domain"),
                body,
            },
        )
        .map_err(|err| {
            site_exit(
                err,
                "Make sure the active identity is a configured tenant site admin for the requested domain.",
            )
        })?;
        self.render_site_result("awiki-cli site root set", &resolved, result)
    }

    pub fn run_site_page_list(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        require_flags(command, &["domain"])?;
        let resolved = self.resolve_config_for_workspace()?;
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli site page list",
                &resolved,
                json!({
                    "plan": {
                        "action": "site.page.list",
                        "identity": self.globals.identity,
                        "rpc_endpoint": "/site/rpc",
                        "rpc_method": "list_pages",
                        "request": {
                            "domain": string_flag(command, "domain").trim(),
                        },
                    }
                }),
                "Dry run: site page list planned",
                Vec::new(),
            );
        }
        let result = site::list_pages(
            &resolved,
            &self.identity_manager(&resolved),
            &string_flag(command, "domain"),
        )
        .map_err(|err| {
            site_exit(
                err,
                "Make sure the active identity is a configured tenant site admin for the requested domain.",
            )
        })?;
        self.render_site_result("awiki-cli site page list", &resolved, result)
    }

    pub fn run_site_page_get(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        require_flags(command, &["domain", "slug"])?;
        let resolved = self.resolve_config_for_workspace()?;
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli site page get",
                &resolved,
                json!({
                    "plan": {
                        "action": "site.page.get",
                        "identity": self.globals.identity,
                        "rpc_endpoint": "/site/rpc",
                        "rpc_method": "get_page",
                        "request": {
                            "domain": string_flag(command, "domain").trim(),
                            "slug": string_flag(command, "slug").trim(),
                        },
                    }
                }),
                "Dry run: site page get planned",
                Vec::new(),
            );
        }
        let result = site::get_page(
            &resolved,
            &self.identity_manager(&resolved),
            &string_flag(command, "domain"),
            &string_flag(command, "slug"),
        )
        .map_err(|err| {
            site_exit(
                err,
                "Make sure the page exists and the active identity can access it.",
            )
        })?;
        self.render_site_result("awiki-cli site page get", &resolved, result)
    }

    pub fn run_site_page_create(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        require_flags(command, &["domain", "slug"])?;
        let body = required_markdown_body(command)?;
        let resolved = self.resolve_config_for_workspace()?;
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli site page create",
                &resolved,
                json!({
                    "plan": {
                        "action": "site.page.create",
                        "identity": self.globals.identity,
                        "rpc_endpoint": "/site/rpc",
                        "rpc_method": "create_page",
                        "request": {
                            "domain": string_flag(command, "domain").trim(),
                            "slug": string_flag(command, "slug").trim(),
                            "body_bytes": body.len(),
                        },
                    }
                }),
                "Dry run: site page create planned",
                Vec::new(),
            );
        }
        let result = site::create_page(
            &resolved,
            &self.identity_manager(&resolved),
            site::CreatePageParams {
                domain: string_flag(command, "domain"),
                slug: string_flag(command, "slug"),
                body,
            },
        )
        .map_err(|err| {
            site_exit(
                err,
                "Make sure the active identity is a configured tenant site admin for the requested domain and the slug is available.",
            )
        })?;
        self.render_site_result("awiki-cli site page create", &resolved, result)
    }

    pub fn run_site_page_update(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        require_flags(command, &["domain", "slug"])?;
        let body = required_markdown_body(command)?;
        let resolved = self.resolve_config_for_workspace()?;
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli site page update",
                &resolved,
                json!({
                    "plan": {
                        "action": "site.page.update",
                        "identity": self.globals.identity,
                        "rpc_endpoint": "/site/rpc",
                        "rpc_method": "update_page",
                        "request": {
                            "domain": string_flag(command, "domain").trim(),
                            "slug": string_flag(command, "slug").trim(),
                            "body_bytes": body.len(),
                        },
                    }
                }),
                "Dry run: site page update planned",
                Vec::new(),
            );
        }
        let result = site::update_page(
            &resolved,
            &self.identity_manager(&resolved),
            site::UpdatePageParams {
                domain: string_flag(command, "domain"),
                slug: string_flag(command, "slug"),
                body,
            },
        )
        .map_err(|err| {
            site_exit(
                err,
                "Make sure the page exists and the active identity can update it.",
            )
        })?;
        self.render_site_result("awiki-cli site page update", &resolved, result)
    }

    pub fn run_site_page_rename(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        require_flags(command, &["domain", "slug", "to"])?;
        let resolved = self.resolve_config_for_workspace()?;
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli site page rename",
                &resolved,
                json!({
                    "plan": {
                        "action": "site.page.rename",
                        "identity": self.globals.identity,
                        "rpc_endpoint": "/site/rpc",
                        "rpc_method": "rename_page",
                        "request": {
                            "domain": string_flag(command, "domain").trim(),
                            "old_slug": string_flag(command, "slug").trim(),
                            "new_slug": string_flag(command, "to").trim(),
                        },
                    }
                }),
                "Dry run: site page rename planned",
                Vec::new(),
            );
        }
        let result = site::rename_page(
            &resolved,
            &self.identity_manager(&resolved),
            site::RenamePageParams {
                domain: string_flag(command, "domain"),
                slug: string_flag(command, "slug"),
                to: string_flag(command, "to"),
            },
        )
        .map_err(|err| {
            site_exit(
                err,
                "Make sure the source page exists and the target slug is available.",
            )
        })?;
        self.render_site_result("awiki-cli site page rename", &resolved, result)
    }

    pub fn run_site_page_delete(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        require_flags(command, &["domain", "slug"])?;
        let resolved = self.resolve_config_for_workspace()?;
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli site page delete",
                &resolved,
                json!({
                    "plan": {
                        "action": "site.page.delete",
                        "identity": self.globals.identity,
                        "rpc_endpoint": "/site/rpc",
                        "rpc_method": "delete_page",
                        "request": {
                            "domain": string_flag(command, "domain").trim(),
                            "slug": string_flag(command, "slug").trim(),
                        },
                    }
                }),
                "Dry run: site page delete planned",
                Vec::new(),
            );
        }
        let result = site::delete_page(
            &resolved,
            &self.identity_manager(&resolved),
            &string_flag(command, "domain"),
            &string_flag(command, "slug"),
        )
        .map_err(|err| {
            site_exit(
                err,
                "Make sure the page exists and the active identity can delete it.",
            )
        })?;
        self.render_site_result("awiki-cli site page delete", &resolved, result)
    }

    fn render_site_result(
        &self,
        command: &str,
        resolved: &crate::config::Resolved,
        result: CommandResult,
    ) -> Result<(), ExitError> {
        self.render_success(
            command,
            resolved,
            result.data,
            &result.summary,
            result.warnings,
        )
    }
}

fn required_markdown_body(command: &ParsedCommand) -> Result<String, ExitError> {
    if !changed_flag(command, "markdown") && !changed_flag(command, "markdown-file") {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "provide either inline markdown or markdown file",
            "Provide --markdown or --markdown-file.",
        ));
    }
    resolve_markdown_body(command)
}

fn resolve_markdown_body(command: &ParsedCommand) -> Result<String, ExitError> {
    let markdown_changed = changed_flag(command, "markdown");
    let markdown_file_changed = changed_flag(command, "markdown-file");
    if markdown_changed && markdown_file_changed {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "use either inline markdown or markdown file, not both",
            "Choose one content body source and make sure the file is readable.",
        ));
    }
    if markdown_file_changed {
        let markdown_file = string_flag(command, "markdown-file");
        return fs::read_to_string(&markdown_file).map_err(|err| {
            ExitError::new(
                "invalid_argument",
                2,
                format!("open {markdown_file}: {err}"),
                "Choose one content body source and make sure the file is readable.",
            )
        });
    }
    Ok(string_flag(command, "markdown"))
}

fn require_flags(command: &ParsedCommand, names: &[&str]) -> Result<(), ExitError> {
    let missing: Vec<_> = names
        .iter()
        .copied()
        .filter(|name| !changed_flag(command, name))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let quoted = missing
        .iter()
        .map(|name| format!("{name:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    Err(ExitError::new(
        "internal_error",
        1,
        format!("required flag(s) {quoted} not set"),
        "",
    ))
}

fn string_flag(command: &ParsedCommand, name: &str) -> String {
    command.flags.get(name).cloned().unwrap_or_default()
}

fn changed_flag(command: &ParsedCommand, name: &str) -> bool {
    command.changed_flags.iter().any(|flag| flag == name)
}

fn site_exit(err: SiteError, hint: &str) -> ExitError {
    match err {
        SiteError::DomainRequired
        | SiteError::DomainInvalid(_)
        | SiteError::SlugRequired
        | SiteError::NoBodySourceProvided
        | SiteError::BodySourceConflict => {
            ExitError::new("invalid_argument", 2, err.to_string(), hint)
        }
        SiteError::AuthIdentityRequired => ExitError::new(
            "auth_required",
            3,
            err.to_string(),
            "Use an identity with a valid JWT, or run `awiki-cli id register` / `awiki-cli id recover` first.",
        ),
        SiteError::Service(service_err) => match () {
            _ if service_err.status_code == 400 || service_err.rpc_code == -32602 => {
                ExitError::new("invalid_argument", 2, service_err.to_string(), hint)
            }
            _ if service_err.status_code == 401 || service_err.rpc_code == -32000 => {
                ExitError::new(
                    "auth_required",
                    3,
                    service_err.to_string(),
                    "Use an identity with a valid JWT or DID WBA auth material.",
                )
            }
            _ if service_err.status_code == 403 || service_err.rpc_code == -32001 => {
                ExitError::new("forbidden", 4, service_err.to_string(), hint)
            }
            _ if service_err.status_code == 404 || service_err.rpc_code == -32002 => {
                ExitError::new("not_found", 5, service_err.to_string(), hint)
            }
            _ if service_err.status_code == 409 || service_err.rpc_code == -32003 => {
                ExitError::new("conflict", 1, service_err.to_string(), hint)
            }
            _ if service_err.rpc_code == -32004 => {
                ExitError::new("invalid_argument", 2, service_err.to_string(), hint)
            }
            _ => ExitError::new("internal_error", 1, service_err.to_string(), hint),
        },
        SiteError::Identity(err) => identity_exit(err),
        SiteError::Internal(message) => ExitError::new("internal_error", 1, message, hint),
    }
}
