use super::{current_schema_version, ensure_schema, open_read_only, query::query_rows};
use super::{ImportReport, LegacyScan, StoreError, StoreResult};
use crate::config::Paths;
use rusqlite::types::{Value as SqlValue, ValueRef};
use rusqlite::{params_from_iter, Connection};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const IMPORT_TABLES: &[&str] = &[
    "messages",
    "e2ee_outbox",
    "contacts",
    "groups",
    "group_members",
    "relationship_events",
    "e2ee_sessions",
];

pub fn scan_legacy_database(paths: &Paths) -> StoreResult<LegacyScan> {
    let legacy_path = legacy_database_path(&paths.legacy_data_dir);
    let mut scan = LegacyScan {
        path: legacy_path.to_string_lossy().into_owned(),
        exists: false,
        schema_version: 0,
        tables: Vec::new(),
    };
    if !legacy_path.exists() {
        return Ok(scan);
    }
    let connection = open_read_only(&scan.path)?;
    scan.exists = true;
    scan.schema_version = current_schema_version(&connection)?;
    let rows = query_rows(
        &connection,
        "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
    )?;
    scan.tables = rows
        .iter()
        .filter_map(|row| row.get("name").and_then(|value| value.as_str()))
        .map(ToOwned::to_owned)
        .collect();
    Ok(scan)
}

pub fn import_legacy_database(target: &mut Connection, paths: &Paths) -> StoreResult<ImportReport> {
    let scan = scan_legacy_database(paths)?;
    if !scan.exists {
        return Err(StoreError::LegacyDatabaseNotFound);
    }
    if scan.schema_version > 0 && scan.schema_version < 6 {
        return Err(StoreError::UnsupportedLegacySchema(
            "unsupported legacy sqlite schema version: legacy schema < 6 requires at least one imported identity so owner_did can be inferred".to_string(),
        ));
    }
    let source = open_read_only(&scan.path)?;
    ensure_schema(target)?;
    let tx = target.transaction()?;
    let mut report = ImportReport {
        source_path: scan.path.clone(),
        source_schema_version: scan.schema_version,
        imported_rows: BTreeMap::new(),
        skipped_tables: Vec::new(),
        warnings: Vec::new(),
    };
    for table in IMPORT_TABLES {
        if !table_exists(&source, table)? {
            report.skipped_tables.push((*table).to_string());
            continue;
        }
        let count = copy_table(&source, &tx, table)?;
        report.imported_rows.insert((*table).to_string(), count);
    }
    report.skipped_tables.sort();
    tx.commit()?;
    Ok(report)
}

fn legacy_database_path(raw: &str) -> PathBuf {
    let path = Path::new(raw.trim());
    if raw.trim().to_ascii_lowercase().ends_with(".db") {
        return path.to_path_buf();
    }
    path.join("database").join("awiki.db")
}

fn table_exists(connection: &Connection, table: &str) -> StoreResult<bool> {
    let count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn copy_table(source: &Connection, target: &Connection, table: &str) -> StoreResult<usize> {
    let mut select = source.prepare(&format!("SELECT * FROM {table}"))?;
    let columns = select
        .column_names()
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if columns.is_empty() {
        return Ok(0);
    }
    let column_list = columns.join(", ");
    let placeholders = (1..=columns.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let insert_sql =
        format!("INSERT OR REPLACE INTO {table} ({column_list}) VALUES ({placeholders})");
    let mut insert = target.prepare(&insert_sql)?;
    let mut rows = select.query([])?;
    let mut count = 0;
    while let Some(row) = rows.next()? {
        let mut values = Vec::with_capacity(columns.len());
        for index in 0..columns.len() {
            values.push(sql_value(row.get_ref(index)?));
        }
        insert.execute(params_from_iter(values.iter()))?;
        count += 1;
    }
    Ok(count)
}

fn sql_value(value: ValueRef<'_>) -> SqlValue {
    match value {
        ValueRef::Null => SqlValue::Null,
        ValueRef::Integer(value) => SqlValue::Integer(value),
        ValueRef::Real(value) => SqlValue::Real(value),
        ValueRef::Text(value) => SqlValue::Text(String::from_utf8_lossy(value).into_owned()),
        ValueRef::Blob(value) => SqlValue::Blob(value.to_vec()),
    }
}
