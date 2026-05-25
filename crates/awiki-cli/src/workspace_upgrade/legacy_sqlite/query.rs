use super::StoreResult;
use rusqlite::types::ValueRef;
use rusqlite::Connection;
use serde_json::{json, Map, Number, Value};

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
