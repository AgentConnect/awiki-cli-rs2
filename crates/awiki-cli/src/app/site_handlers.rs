use super::{not_implemented_side_effect, App};
use crate::cli::ParsedCommand;
use crate::output::ExitError;
use serde_json::json;
use std::fs;

impl App {
    pub fn run_site_root_get(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        require_flags(command, &["domain"])?;
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            return Err(not_implemented_side_effect("site root get"));
        }
        self.render_success(
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
        )
    }

    pub fn run_site_root_set(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        require_flags(command, &["domain"])?;
        let body = required_markdown_body(command)?;
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            return Err(not_implemented_side_effect("site root set"));
        }
        self.render_success(
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
        )
    }

    pub fn run_site_page_list(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        require_flags(command, &["domain"])?;
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            return Err(not_implemented_side_effect("site page list"));
        }
        self.render_success(
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
        )
    }

    pub fn run_site_page_get(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        require_flags(command, &["domain", "slug"])?;
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            return Err(not_implemented_side_effect("site page get"));
        }
        self.render_success(
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
        )
    }

    pub fn run_site_page_create(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        require_flags(command, &["domain", "slug"])?;
        let body = required_markdown_body(command)?;
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            return Err(not_implemented_side_effect("site page create"));
        }
        self.render_success(
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
        )
    }

    pub fn run_site_page_update(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        require_flags(command, &["domain", "slug"])?;
        let body = required_markdown_body(command)?;
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            return Err(not_implemented_side_effect("site page update"));
        }
        self.render_success(
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
        )
    }

    pub fn run_site_page_rename(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        require_flags(command, &["domain", "slug", "to"])?;
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            return Err(not_implemented_side_effect("site page rename"));
        }
        self.render_success(
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
        )
    }

    pub fn run_site_page_delete(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        require_flags(command, &["domain", "slug"])?;
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            return Err(not_implemented_side_effect("site page delete"));
        }
        self.render_success(
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
        .filter(|name| string_flag(command, name).trim().is_empty())
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
