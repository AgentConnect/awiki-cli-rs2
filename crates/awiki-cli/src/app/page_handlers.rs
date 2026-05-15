use super::{identity_exit, App};
use crate::cli::ParsedCommand;
use crate::content::{self, CommandResult, ContentError};
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
        let resolved = self.resolve_config()?;
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli page create",
                &resolved,
                json!({
                "plan": {
                    "action": "page.create",
                    "identity": self.globals.identity,
                    "rpc_endpoint": "/content/rpc",
                    "rpc_method": "create",
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
        let result = content::create_page(
            &resolved,
            &self.identity_manager(&resolved),
            content::CreatePageParams {
                slug,
                title,
                body,
                visibility: string_flag(command, "visibility"),
            },
        )
        .map_err(|err| {
            content_exit(
                err,
                "Make sure the active identity has a handle and the page slug is valid.",
            )
        })?;
        self.render_content_result("awiki-cli page create", &resolved, result)
    }

    pub fn run_page_list(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli page list",
                &resolved,
                json!({
                    "plan": {
                        "action": "page.list",
                        "identity": self.globals.identity,
                        "rpc_endpoint": "/content/rpc",
                        "rpc_method": "list",
                    }
                }),
                "Dry run: page list planned",
                Vec::new(),
            );
        }
        let result =
            content::list_pages(&resolved, &self.identity_manager(&resolved)).map_err(|err| {
                content_exit(
                    err,
                    "Make sure the active identity has a handle and can access content pages.",
                )
            })?;
        self.render_content_result("awiki-cli page list", &resolved, result)
    }

    pub fn run_page_get(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli page get",
                &resolved,
                json!({
                    "plan": {
                        "action": "page.get",
                        "identity": self.globals.identity,
                        "rpc_endpoint": "/content/rpc",
                        "rpc_method": "get",
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
        let result = content::get_page(&resolved, &self.identity_manager(&resolved), &slug)
            .map_err(|err| {
                content_exit(
                    err,
                    "Make sure the page exists and the active identity can access it.",
                )
            })?;
        self.render_content_result("awiki-cli page get", &resolved, result)
    }

    pub fn run_page_update(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let body = resolve_markdown_body(command)?;
        let resolved = self.resolve_config()?;
        let title = string_flag(command, "title");
        if !self.globals.dry_run {
            let result = content::update_page(
                &resolved,
                &self.identity_manager(&resolved),
                content::UpdatePageParams {
                    slug: string_flag(command, "slug"),
                    title,
                    body,
                    visibility: changed_flag(command, "visibility")
                        .then(|| string_flag(command, "visibility")),
                },
            )
            .map_err(|err| {
                content_exit(
                    err,
                    "Make sure the page exists and the updated fields are valid.",
                )
            })?;
            return self.render_content_result("awiki-cli page update", &resolved, result);
        }
        let mut changed_fields = Vec::new();
        if !title.trim().is_empty() {
            changed_fields.push(Value::String("title".to_string()));
        }
        if body.is_some() {
            changed_fields.push(Value::String("body".to_string()));
        }
        if changed_flag(command, "visibility") {
            changed_fields.push(Value::String("visibility".to_string()));
        }
        let body_bytes = body.as_ref().map(|value| value.len()).unwrap_or_default();
        self.render_success(
            "awiki-cli page update",
            &resolved,
            json!({
                "plan": {
                    "action": "page.update",
                    "identity": self.globals.identity,
                    "rpc_endpoint": "/content/rpc",
                    "rpc_method": "update",
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
        let resolved = self.resolve_config()?;
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli page rename",
                &resolved,
                json!({
                    "plan": {
                        "action": "page.rename",
                        "identity": self.globals.identity,
                        "rpc_endpoint": "/content/rpc",
                        "rpc_method": "rename",
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
        let result = content::rename_page(
            &resolved,
            &self.identity_manager(&resolved),
            content::RenamePageParams {
                slug: string_flag(command, "slug"),
                to: string_flag(command, "to"),
            },
        )
        .map_err(|err| {
            content_exit(
                err,
                "Make sure the source page exists and the target slug is available.",
            )
        })?;
        self.render_content_result("awiki-cli page rename", &resolved, result)
    }

    pub fn run_page_delete(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli page delete",
                &resolved,
                json!({
                    "plan": {
                        "action": "page.delete",
                        "identity": self.globals.identity,
                        "rpc_endpoint": "/content/rpc",
                        "rpc_method": "delete",
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
        let result = content::delete_page(&resolved, &self.identity_manager(&resolved), &slug)
            .map_err(|err| {
                content_exit(
                    err,
                    "Make sure the page exists and the active identity can delete it.",
                )
            })?;
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

fn content_exit(err: ContentError, hint: &str) -> ExitError {
    match err {
        ContentError::SlugRequired
        | ContentError::TitleRequired
        | ContentError::NoUpdateFields
        | ContentError::VisibilityInvalid => {
            ExitError::new("invalid_argument", 2, err.to_string(), hint)
        }
        ContentError::BodySourceConflict => ExitError::new(
            "invalid_argument",
            2,
            err.to_string(),
            "Choose one content body source and make sure the file is readable.",
        ),
        ContentError::AuthIdentityRequired => ExitError::new(
            "auth_required",
            3,
            err.to_string(),
            "Use an identity with a valid JWT, or run `awiki-cli id register` / `awiki-cli id recover` first.",
        ),
        ContentError::Service(service_err) => match () {
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
            _ if service_err.status_code == 404 || service_err.rpc_code == -32002 => {
                ExitError::new("not_found", 5, service_err.to_string(), hint)
            }
            _ if service_err.status_code == 409
                || matches!(service_err.rpc_code, -32003 | -32004) =>
            {
                ExitError::new("conflict", 1, service_err.to_string(), hint)
            }
            _ => ExitError::new("internal_error", 1, service_err.to_string(), hint),
        },
        ContentError::Identity(err) => identity_exit(err),
        ContentError::Internal(message) => ExitError::new("internal_error", 1, message, hint),
    }
}
