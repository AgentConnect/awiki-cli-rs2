use serde::Serialize;
use serde_json::{Map, Value};

pub fn struct_to_map<T>(value: T) -> Value
where
    T: Serialize,
{
    match serde_json::to_value(value) {
        Ok(Value::Object(object)) => Value::Object(object),
        Ok(Value::Null) => Value::Null,
        Ok(_) | Err(_) => Value::Object(Map::new()),
    }
}
