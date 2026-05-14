use super::{not_implemented_side_effect, App};
use crate::cli::ParsedCommand;
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
        if !self.globals.dry_run {
            return Err(not_implemented_side_effect("page create"));
        }
        self.render_success(
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
        )
    }

    pub fn run_page_list(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            return Err(not_implemented_side_effect("page list"));
        }
        self.render_success(
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
        )
    }

    pub fn run_page_get(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            return Err(not_implemented_side_effect("page get"));
        }
        self.render_success(
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
        )
    }

    pub fn run_page_update(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let body = resolve_markdown_body(command)?;
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            return Err(not_implemented_side_effect("page update"));
        }
        let mut changed_fields = Vec::new();
        let title = string_flag(command, "title");
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
        if !self.globals.dry_run {
            return Err(not_implemented_side_effect("page rename"));
        }
        self.render_success(
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
        )
    }

    pub fn run_page_delete(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            return Err(not_implemented_side_effect("page delete"));
        }
        self.render_success(
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
