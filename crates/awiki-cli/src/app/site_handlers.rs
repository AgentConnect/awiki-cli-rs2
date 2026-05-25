use super::App;
use crate::cli::ParsedCommand;
use crate::im_core_adapter::site::{self, CommandResult};
use crate::output::ExitError;
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
                        "service": "im-core.site",
                        "operation": "site.root.get",
                        "remote_call": "site.get_root",
                        "identity": self.globals.identity,
                        "request": {
                            "domain": string_flag(command, "domain").trim(),
                        },
                    }
                }),
                "Dry run: site root get planned",
                Vec::new(),
            );
        }
        let client = self.site_client(&resolved)?;
        let result = site::get_root(&client, string_flag(command, "domain")).map_err(site_exit(
            "site root get",
            "Make sure the active identity is a configured tenant site admin for the requested domain.",
        ))?;
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
                        "service": "im-core.site",
                        "operation": "site.root.set",
                        "remote_call": "site.set_root",
                        "identity": self.globals.identity,
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
        let client = self.site_client(&resolved)?;
        let result = site::set_root(&client, string_flag(command, "domain"), body).map_err(
            site_exit(
                "site root set",
                "Make sure the active identity is a configured tenant site admin for the requested domain.",
            ),
        )?;
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
                        "service": "im-core.site",
                        "operation": "site.page.list",
                        "remote_call": "site.list_pages",
                        "identity": self.globals.identity,
                        "request": {
                            "domain": string_flag(command, "domain").trim(),
                        },
                    }
                }),
                "Dry run: site page list planned",
                Vec::new(),
            );
        }
        let client = self.site_client(&resolved)?;
        let result = site::list_pages(&client, string_flag(command, "domain")).map_err(
            site_exit(
                "site page list",
                "Make sure the active identity is a configured tenant site admin for the requested domain.",
            ),
        )?;
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
                        "service": "im-core.site",
                        "operation": "site.page.get",
                        "remote_call": "site.get_page",
                        "identity": self.globals.identity,
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
        let client = self.site_client(&resolved)?;
        let result = site::get_page(
            &client,
            string_flag(command, "domain"),
            string_flag(command, "slug"),
        )
        .map_err(site_exit(
            "site page get",
            "Make sure the page exists and the active identity can access it.",
        ))?;
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
                        "service": "im-core.site",
                        "operation": "site.page.create",
                        "remote_call": "site.create_page",
                        "identity": self.globals.identity,
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
        let client = self.site_client(&resolved)?;
        let result = site::create_page(
            &client,
            string_flag(command, "domain"),
            string_flag(command, "slug"),
            body,
        )
        .map_err(site_exit(
            "site page create",
            "Make sure the active identity is a configured tenant site admin for the requested domain and the slug is available.",
        ))?;
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
                        "service": "im-core.site",
                        "operation": "site.page.update",
                        "remote_call": "site.update_page",
                        "identity": self.globals.identity,
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
        let client = self.site_client(&resolved)?;
        let result = site::update_page(
            &client,
            string_flag(command, "domain"),
            string_flag(command, "slug"),
            body,
        )
        .map_err(site_exit(
            "site page update",
            "Make sure the page exists and the active identity can update it.",
        ))?;
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
                        "service": "im-core.site",
                        "operation": "site.page.rename",
                        "remote_call": "site.rename_page",
                        "identity": self.globals.identity,
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
        let client = self.site_client(&resolved)?;
        let result = site::rename_page(
            &client,
            string_flag(command, "domain"),
            string_flag(command, "slug"),
            string_flag(command, "to"),
        )
        .map_err(site_exit(
            "site page rename",
            "Make sure the source page exists and the target slug is available.",
        ))?;
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
                        "service": "im-core.site",
                        "operation": "site.page.delete",
                        "remote_call": "site.delete_page",
                        "identity": self.globals.identity,
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
        let client = self.site_client(&resolved)?;
        let result = site::delete_page(
            &client,
            string_flag(command, "domain"),
            string_flag(command, "slug"),
        )
        .map_err(site_exit(
            "site page delete",
            "Make sure the page exists and the active identity can delete it.",
        ))?;
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

    fn site_client(
        &self,
        resolved: &crate::config::Resolved,
    ) -> Result<im_core::ImClient, ExitError> {
        let manager = self.identity_manager(resolved);
        crate::im_core_adapter::build_im_client(
            resolved,
            &manager,
            crate::im_core_adapter::cli_identity_selector(&self.globals.identity),
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
        "invalid_argument",
        2,
        format!("required flag(s) {quoted} not set"),
        "Provide all required flags for this site command.",
    ))
}

fn string_flag(command: &ParsedCommand, name: &str) -> String {
    command.flags.get(name).cloned().unwrap_or_default()
}

fn changed_flag(command: &ParsedCommand, name: &str) -> bool {
    command.changed_flags.iter().any(|flag| flag == name)
}

fn site_exit(
    context: &'static str,
    hint: &'static str,
) -> impl FnOnce(im_core::ImError) -> ExitError {
    move |err| match err {
        im_core::ImError::Service {
            status_code,
            code,
            message,
        } => service_exit(context, hint, status_code, code, message),
        err => crate::im_core_adapter::map_im_error(err, context),
    }
}

fn service_exit(
    context: &'static str,
    hint: &'static str,
    status_code: Option<u16>,
    code: Option<String>,
    message: String,
) -> ExitError {
    let rpc_code = code.as_deref().and_then(|value| value.parse::<i64>().ok());
    match () {
        _ if status_code == Some(400) || rpc_code == Some(-32602) => {
            ExitError::new("invalid_argument", 2, message, hint)
        }
        _ if status_code == Some(401) || rpc_code == Some(-32000) => ExitError::new(
            "auth_required",
            3,
            message,
            "Use an identity with a valid JWT or DID WBA auth material.",
        ),
        _ if status_code == Some(403) || rpc_code == Some(-32001) => {
            ExitError::new("forbidden", 4, message, hint)
        }
        _ if status_code == Some(404) || rpc_code == Some(-32002) => {
            ExitError::new("not_found", 5, message, hint)
        }
        _ if status_code == Some(409) || rpc_code == Some(-32003) => {
            ExitError::new("conflict", 1, message, hint)
        }
        _ if rpc_code == Some(-32004) => ExitError::new("invalid_argument", 2, message, hint),
        _ => crate::im_core_adapter::map_im_error(
            im_core::ImError::Service {
                status_code,
                code,
                message,
            },
            context,
        ),
    }
}
