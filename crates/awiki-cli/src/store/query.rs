use super::{StoreError, StoreResult};
use rusqlite::types::ValueRef;
use rusqlite::Connection;
use serde_json::{json, Map, Number, Value};

pub fn execute_sql(connection: &Connection, statement: &str) -> StoreResult<Vec<Value>> {
    let sql_text = normalize_statement(statement)?;
    validate_statement(&sql_text)?;
    if starts_with_keyword(&sql_text, "SELECT") {
        return query_rows(connection, &sql_text);
    }
    let rows_affected = connection.execute(&sql_text, [])?;
    Ok(vec![json!({ "rows_affected": rows_affected })])
}

pub fn list_notifications(
    connection: &Connection,
    owner_did: &str,
    limit: i64,
) -> StoreResult<Vec<Value>> {
    let limit = if limit <= 0 { 20 } else { limit };
    let mut statement = connection.prepare(
        r#"
SELECT *
FROM messages
WHERE owner_did = ?1
  AND (COALESCE(content_type, '') = 'mail.notification'
       OR COALESCE(metadata, '') LIKE '%"source_kind":"mail"%')
ORDER BY COALESCE(sent_at, stored_at) DESC
LIMIT ?2
"#,
    )?;
    let names = statement
        .column_names()
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let mut rows = statement.query(rusqlite::params![owner_did.trim(), limit])?;
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        let mut object = Map::new();
        for (index, name) in names.iter().enumerate() {
            object.insert(name.clone(), value_ref_to_json(row.get_ref(index)?));
        }
        results.push(Value::Object(object));
    }
    Ok(results)
}

pub(crate) fn query_rows(connection: &Connection, statement: &str) -> StoreResult<Vec<Value>> {
    let mut statement = connection.prepare(statement)?;
    let names = statement
        .column_names()
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let mut rows = statement.query([])?;
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        let mut object = Map::new();
        for (index, name) in names.iter().enumerate() {
            object.insert(name.clone(), value_ref_to_json(row.get_ref(index)?));
        }
        results.push(Value::Object(object));
    }
    Ok(results)
}

fn normalize_statement(statement: &str) -> StoreResult<String> {
    let mut sql_text = statement.trim().to_string();
    if sql_text.ends_with(';') {
        sql_text.pop();
        sql_text = sql_text.trim().to_string();
    }
    if sql_text.is_empty() {
        return Err(StoreError::unsafe_sql("empty statement"));
    }
    if sql_text.contains(';') {
        return Err(StoreError::unsafe_sql(
            "multiple statements are not allowed",
        ));
    }
    Ok(sql_text)
}

fn validate_statement(sql_text: &str) -> StoreResult<()> {
    if contains_keyword(sql_text, "DROP") || contains_keyword(sql_text, "TRUNCATE") {
        return Err(StoreError::unsafe_sql("forbidden SQL operation"));
    }
    if starts_with_keyword(sql_text, "DELETE") && !contains_keyword(sql_text, "WHERE") {
        return Err(StoreError::unsafe_sql(
            "DELETE without WHERE clause is not allowed",
        ));
    }
    Ok(())
}

fn starts_with_keyword(text: &str, keyword: &str) -> bool {
    let trimmed = text.trim_start();
    if trimmed.len() < keyword.len() {
        return false;
    }
    let head = &trimmed[..keyword.len()];
    head.eq_ignore_ascii_case(keyword)
        && trimmed[keyword.len()..]
            .chars()
            .next()
            .map(is_boundary)
            .unwrap_or(true)
}

fn contains_keyword(text: &str, keyword: &str) -> bool {
    let upper = text.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    let needle = keyword.as_bytes();
    if needle.is_empty() || bytes.len() < needle.len() {
        return false;
    }
    for index in 0..=bytes.len() - needle.len() {
        if &bytes[index..index + needle.len()] != needle {
            continue;
        }
        let before = if index == 0 {
            true
        } else {
            is_boundary(bytes[index - 1] as char)
        };
        let after_index = index + needle.len();
        let after = if after_index >= bytes.len() {
            true
        } else {
            is_boundary(bytes[after_index] as char)
        };
        if before && after {
            return true;
        }
    }
    false
}

fn is_boundary(ch: char) -> bool {
    !(ch.is_ascii_alphanumeric() || ch == '_')
}

fn value_ref_to_json(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(value) => json!(value),
        ValueRef::Real(value) => Number::from_f64(value)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        ValueRef::Text(value) => Value::String(String::from_utf8_lossy(value).into_owned()),
        ValueRef::Blob(value) => Value::String(String::from_utf8_lossy(value).into_owned()),
    }
}
