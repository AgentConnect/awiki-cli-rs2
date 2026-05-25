use super::{legacy_owner_lookup, store_exit, App};
use crate::cli::ParsedCommand;
use crate::legacy_store::{self as store, StoreError};
use crate::output::ExitError;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

impl App {
    pub fn run_debug_db_query(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        if command.args.len() != 1 {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "debug db query requires exactly one SQL statement.",
                "Usage: awiki-cli debug db query \"SELECT * FROM messages LIMIT 5\"",
            ));
        }
        let resolved = self.resolve_config_for_workspace()?;
        let db = self.open_store(
            &resolved,
            "Run `awiki-cli doctor` to inspect the database path and configuration.",
        )?;
        store::ensure_schema(&db)
            .map_err(|err| store_exit(err, "Initialize the local store before querying it."))?;
        let rows = store::execute_sql(&db, &command.args[0]).map_err(|err| {
            store_exit(
                err,
                "Only single-statement safe SQL is allowed. Avoid destructive statements.",
            )
        })?;
        self.render_success(
            "awiki-cli debug db query",
            &resolved,
            json!({ "database_file": resolved.paths.database_file, "rows": rows }),
            "SQLite query executed",
            Vec::new(),
        )
    }

    pub fn run_debug_db_import_v1(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let mut db = self.open_store(
            &resolved,
            "Run `awiki-cli doctor` to inspect the database path and configuration.",
        )?;
        store::ensure_schema(&db).map_err(|err| {
            store_exit(
                err,
                "Initialize the local store before importing legacy data.",
            )
        })?;
        let mut paths = resolved.paths.clone();
        if let Some(path) = command
            .flags
            .get("path")
            .filter(|value| !value.trim().is_empty())
        {
            paths.legacy_data_dir = path.trim().to_string();
        }
        if self.globals.dry_run {
            let scan = store::scan_legacy_database(&paths)
                .map_err(|err| store_exit(err, "Make sure the legacy database path is correct."))?;
            return self.render_success(
                "awiki-cli debug db import-v1",
                &resolved,
                json!({
                    "plan": {
                        "action": "import_v1_sqlite",
                        "source_scan": scan,
                        "target": resolved.paths.database_file,
                    }
                }),
                "Dry run: legacy SQLite import planned",
                Vec::new(),
            );
        }
        let owners = legacy_owner_lookup(&self.identity_manager(&resolved));
        let report = store::import_legacy_database(&mut db, &paths, &owners).map_err(|err| {
            store_exit(
                err,
                "Make sure the v1 database exists and identities were imported first.",
            )
        })?;
        let warnings = report.warnings.clone();
        self.render_success(
            "awiki-cli debug db import-v1",
            &resolved,
            json!({ "database_file": resolved.paths.database_file, "import_report": report }),
            "Legacy SQLite import completed",
            warnings,
        )
    }

    pub fn run_debug_db_handle_history(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        if command.args.len() != 1 {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "debug db handle-history requires exactly one handle.",
                "Usage: awiki-cli debug db handle-history <handle>",
            ));
        }
        let resolved = self.resolve_config_for_workspace()?;
        let db = self.open_store(
            &resolved,
            "Run `awiki-cli doctor` to inspect the database path and configuration.",
        )?;
        store::ensure_schema(&db).map_err(|err| {
            store_exit(
                err,
                "Initialize the local store before querying handle history.",
            )
        })?;

        let handle = normalize_debug_handle(&command.args[0]);
        if handle.is_empty() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "handle is required.",
                "Provide a handle local-part or full handle.",
            ));
        }

        let rows = store::list_contact_handle_history(&db, &handle).map_err(|err| {
            store_exit(
                err,
                "Make sure the local store schema is current before reading handle history.",
            )
        })?;
        if rows.is_empty() {
            return Err(store_exit(
                StoreError::NotFound("sql: no rows in result set".to_string()),
                &format!("No local DID history is stored for handle {handle:?}."),
            ));
        }

        self.render_success(
            "awiki-cli debug db handle-history",
            &resolved,
            json!({
                "database_file": resolved.paths.database_file,
                "handle": handle,
                "owners": build_handle_history_owners(&rows),
                "rows": rows,
            }),
            &format!("Loaded local DID history for handle {handle}"),
            Vec::new(),
        )
    }
}

fn normalize_debug_handle(raw: &str) -> String {
    let mut value = raw.trim().to_ascii_lowercase();
    if let Some(stripped) = value.strip_prefix("wba://") {
        value = stripped.to_string();
    }
    if let Some(index) = value.find('.') {
        if index > 0 {
            return value[..index].to_string();
        }
    }
    value
}

fn build_handle_history_owners(rows: &[Value]) -> Vec<Value> {
    let mut current_by_owner = BTreeMap::<String, String>::new();
    let mut historical_by_owner = BTreeMap::<String, Vec<String>>::new();
    for row in rows {
        let owner_did = string_field(row, "owner_did").trim().to_string();
        let did = string_field(row, "did").trim().to_string();
        if owner_did.is_empty() || did.is_empty() {
            continue;
        }
        if bool_field(row, "is_current") {
            current_by_owner.insert(owner_did.clone(), did.clone());
        }
        historical_by_owner.entry(owner_did).or_default().push(did);
    }

    let mut seen = BTreeSet::<String>::new();
    let mut owners = Vec::new();
    for row in rows {
        let owner_did = string_field(row, "owner_did").trim().to_string();
        if owner_did.is_empty() || !seen.insert(owner_did.clone()) {
            continue;
        }
        let historical_dids = historical_by_owner
            .get(&owner_did)
            .cloned()
            .unwrap_or_default();
        owners.push(json!({
            "owner_did": owner_did,
            "current_did": current_by_owner.get(&owner_did).cloned().unwrap_or_default(),
            "historical_dids": historical_dids,
            "historical_count": historical_by_owner.get(&owner_did).map(Vec::len).unwrap_or(0),
        }));
    }
    owners
}

fn string_field(row: &Value, field: &str) -> String {
    row.get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn bool_field(row: &Value, field: &str) -> bool {
    match row.get(field) {
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(value)) => value.as_i64().is_some_and(|number| number != 0),
        Some(Value::String(value)) => value == "1" || value.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_debug_handle_trims_prefixes_and_domains() {
        let cases = [
            (" Alice.AWiki.ai ", "alice"),
            ("wba://Bob.example.com", "bob"),
            ("carol", "carol"),
            ("", ""),
        ];

        for (input, expected) in cases {
            assert_eq!(normalize_debug_handle(input), expected);
        }
    }

    #[test]
    fn build_handle_history_owners_aggregates_by_first_row_order() {
        let rows = vec![
            json!({"owner_did": "did:owner-a", "did": "did:peer-current", "is_current": 1}),
            json!({"owner_did": "did:owner-a", "did": "did:peer-old", "is_current": 0}),
            json!({"owner_did": "did:owner-b", "did": "did:peer-b", "is_current": "true"}),
        ];

        let owners = build_handle_history_owners(&rows);

        assert_eq!(owners.len(), 2);
        assert_eq!(owners[0]["owner_did"], "did:owner-a");
        assert_eq!(owners[0]["current_did"], "did:peer-current");
        assert_eq!(owners[0]["historical_count"], 2);
        assert_eq!(owners[1]["owner_did"], "did:owner-b");
        assert_eq!(owners[1]["current_did"], "did:peer-b");
    }
}
