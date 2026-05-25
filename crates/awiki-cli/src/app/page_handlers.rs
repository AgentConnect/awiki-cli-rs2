use super::App;
use crate::cli::ParsedCommand;
use crate::im_core_adapter::content::{self, CommandResult};
use crate::output::ExitError;
use serde_json::{json, Value};
use std::fs;

impl App {
    pub fn run_page_create(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let slug = string_flag(command, "slug");
        let title = string_flag(command, "title");
        if slug.trim().is_empty() {
            return Err(invalid_page_arg("slug is required", "slug is required"));
        }
        if title.trim().is_empty() {
            return Err(invalid_page_arg("title is required", "title is required"));
        }
        let body = resolve_markdown_body(command)?.unwrap_or_default();
        let resolved = self.resolve_config_for_workspace()?;
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli page create",
                &resolved,
                json!({
                "plan": {
                    "action": "page.create",
                    "service": "im-core.content",
                    "operation": "page.create",
                    "remote_call": "content.create_page",
                    "identity": self.globals.identity,
                    "request": {
                        "slug": slug.trim(),
                        "title": title.trim(),
                        "body_bytes": body.len(),
                        "visibility": default_string(&string_flag(command, "visibility"), "public"),
                    },
                }
            }),
                "Dry run: page create planned",
                Vec::new(),
            );
        }
        let client = self.content_client(&resolved)?;
        let result = content::create_page(
            &client,
            slug,
            title,
            body,
            string_flag(command, "visibility"),
        )
        .map_err(content_exit(
            "page create",
            "Make sure the active identity has a handle and the page slug is valid.",
        ))?;
        self.render_content_result("awiki-cli page create", &resolved, result)
    }

    pub fn run_page_list(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli page list",
                &resolved,
                json!({
                    "plan": {
                        "action": "page.list",
                        "service": "im-core.content",
                        "operation": "page.list",
                        "remote_call": "content.list_pages",
                        "identity": self.globals.identity,
                    }
                }),
                "Dry run: page list planned",
                Vec::new(),
            );
        }
        let client = self.content_client(&resolved)?;
        let result = content::list_pages(&client).map_err(content_exit(
            "page list",
            "Make sure the active identity has a handle and can access content pages.",
        ))?;
        self.render_content_result("awiki-cli page list", &resolved, result)
    }

    pub fn run_page_get(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        require_flags(command, &["slug"])?;
        let resolved = self.resolve_config_for_workspace()?;
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli page get",
                &resolved,
                json!({
                    "plan": {
                        "action": "page.get",
                        "service": "im-core.content",
                        "operation": "page.get",
                        "remote_call": "content.get_page",
                        "identity": self.globals.identity,
                        "request": {
                            "slug": string_flag(command, "slug").trim(),
                        },
                    }
                }),
                "Dry run: page get planned",
                Vec::new(),
            );
        }
        let slug = string_flag(command, "slug");
        let client = self.content_client(&resolved)?;
        let result = content::get_page(&client, slug).map_err(content_exit(
            "page get",
            "Make sure the page exists and the active identity can access it.",
        ))?;
        self.render_content_result("awiki-cli page get", &resolved, result)
    }

    pub fn run_page_update(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        require_flags(command, &["slug"])?;
        let body = resolve_markdown_body(command)?;
        let resolved = self.resolve_config_for_workspace()?;
        let title = string_flag(command, "title");
        if !self.globals.dry_run {
            let client = self.content_client(&resolved)?;
            let result = content::update_page(
                &client,
                string_flag(command, "slug"),
                title,
                body,
                changed_flag(command, "visibility").then(|| string_flag(command, "visibility")),
            )
            .map_err(content_exit(
                "page update",
                "Make sure the page exists and the updated fields are valid.",
            ))?;
            return self.render_content_result("awiki-cli page update", &resolved, result);
        }
        let mut changed_fields = Vec::new();
        if !title.trim().is_empty() {
            changed_fields.push(Value::String("title".to_string()));
        }
        if body.is_some() {
            changed_fields.push(Value::String("body".to_string()));
        }
        if changed_flag(command, "visibility")
            && !string_flag(command, "visibility").trim().is_empty()
        {
            changed_fields.push(Value::String("visibility".to_string()));
        }
        let body_bytes = body.as_ref().map(|value| value.len()).unwrap_or_default();
        self.render_success(
            "awiki-cli page update",
            &resolved,
            json!({
                "plan": {
                    "action": "page.update",
                    "service": "im-core.content",
                    "operation": "page.update",
                    "remote_call": "content.update_page",
                    "identity": self.globals.identity,
                    "changed_fields": changed_fields,
                    "request": {
                        "slug": string_flag(command, "slug").trim(),
                        "title": title.trim(),
                        "body_bytes": body_bytes,
                        "visibility": string_flag(command, "visibility"),
                    },
                }
            }),
            "Dry run: page update planned",
            Vec::new(),
        )
    }

    pub fn run_page_rename(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        require_flags(command, &["slug", "to"])?;
        let resolved = self.resolve_config_for_workspace()?;
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli page rename",
                &resolved,
                json!({
                    "plan": {
                        "action": "page.rename",
                        "service": "im-core.content",
                        "operation": "page.rename",
                        "remote_call": "content.rename_page",
                        "identity": self.globals.identity,
                        "request": {
                            "old_slug": string_flag(command, "slug").trim(),
                            "new_slug": string_flag(command, "to").trim(),
                        },
                    }
                }),
                "Dry run: page rename planned",
                Vec::new(),
            );
        }
        let client = self.content_client(&resolved)?;
        let result = content::rename_page(
            &client,
            string_flag(command, "slug"),
            string_flag(command, "to"),
        )
        .map_err(content_exit(
            "page rename",
            "Make sure the source page exists and the target slug is available.",
        ))?;
        self.render_content_result("awiki-cli page rename", &resolved, result)
    }

    pub fn run_page_delete(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        require_flags(command, &["slug"])?;
        let resolved = self.resolve_config_for_workspace()?;
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli page delete",
                &resolved,
                json!({
                    "plan": {
                        "action": "page.delete",
                        "service": "im-core.content",
                        "operation": "page.delete",
                        "remote_call": "content.delete_page",
                        "identity": self.globals.identity,
                        "request": {
                            "slug": string_flag(command, "slug").trim(),
                        },
                    }
                }),
                "Dry run: page delete planned",
                Vec::new(),
            );
        }
        let slug = string_flag(command, "slug");
        let client = self.content_client(&resolved)?;
        let result = content::delete_page(&client, slug).map_err(content_exit(
            "page delete",
            "Make sure the page exists and the active identity can delete it.",
        ))?;
        self.render_content_result("awiki-cli page delete", &resolved, result)
    }

    fn render_content_result(
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

    fn content_client(
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

fn resolve_markdown_body(command: &ParsedCommand) -> Result<Option<String>, ExitError> {
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
        return fs::read_to_string(&markdown_file).map(Some).map_err(|err| {
            invalid_page_arg(
                format!("open {markdown_file}: {err}"),
                "Choose one content body source and make sure the file is readable.",
            )
        });
    }
    if markdown_changed {
        return Ok(Some(string_flag(command, "markdown")));
    }
    Ok(None)
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
        "Provide all required flags for this page command.",
    ))
}

fn string_flag(command: &ParsedCommand, name: &str) -> String {
    command.flags.get(name).cloned().unwrap_or_default()
}

fn default_string(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn changed_flag(command: &ParsedCommand, name: &str) -> bool {
    command.changed_flags.iter().any(|flag| flag == name)
}

fn invalid_page_arg(message: impl Into<String>, hint: impl Into<String>) -> ExitError {
    ExitError::new("invalid_argument", 2, message, hint)
}

fn content_exit(
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
        _ if status_code == Some(404) || rpc_code == Some(-32002) => {
            ExitError::new("not_found", 5, message, hint)
        }
        _ if status_code == Some(409) || matches!(rpc_code, Some(-32003 | -32004)) => {
            ExitError::new("conflict", 1, message, hint)
        }
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
