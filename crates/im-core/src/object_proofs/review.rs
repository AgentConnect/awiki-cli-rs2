use crate::{ImError, ImResult};
use serde::{de, Deserialize, Deserializer};
use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};
use std::fmt;
const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
const MAX_OBJECT_BYTES: usize = 1024 * 1024;

/// Frozen unsigned object. It cannot be deserialized with a caller-supplied digest.
/// Applications validate their own schema before preparing this object.
pub struct ObjectProofReview {
    pub(super) object: Value,
    object_hash: String,
}
impl ObjectProofReview {
    pub fn parse(raw_json: &[u8], expected_object_hash: &str) -> ImResult<Self> {
        if raw_json.len() > MAX_OBJECT_BYTES {
            return Err(invalid());
        }
        let StrictValue(mut object) = serde_json::from_slice(raw_json).map_err(|_| invalid())?;
        let map = object.as_object_mut().ok_or_else(invalid)?;
        // Standard ANP Object Proof signs the object without the ENTIRE top-level proof.
        map.remove("proof");
        let canonical = serde_json_canonicalizer::to_vec(&object).map_err(|_| invalid())?;
        let object_hash = format!("{:x}", Sha256::digest(&canonical));
        if object_hash != expected_object_hash {
            return Err(invalid());
        }
        Ok(Self {
            object,
            object_hash,
        })
    }
    pub fn object_hash(&self) -> &str {
        &self.object_hash
    }
    /// JSON string escaping keeps terminal control characters inert.
    pub fn presentation(&self) -> Value {
        serde_json::json!({"object":self.object,"object_hash":self.object_hash})
    }
}
fn invalid() -> ImError {
    ImError::invalid_input(
        Some("object_proof".to_owned()),
        "invalid JSON object or object digest",
    )
}

struct StrictValue(Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Visitor;
        impl<'de> de::Visitor<'de> for Visitor {
            type Value = StrictValue;
            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("JSON with unique keys, safe integers and no NUL")
            }
            fn visit_bool<E: de::Error>(self, value: bool) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::Bool(value)))
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> std::result::Result<Self::Value, E> {
                if !(-MAX_SAFE_INTEGER..=MAX_SAFE_INTEGER).contains(&value) {
                    return Err(E::custom("integer outside the JCS safe range"));
                }
                Ok(StrictValue(Value::Number(Number::from(value))))
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> std::result::Result<Self::Value, E> {
                if value > MAX_SAFE_INTEGER as u64 {
                    return Err(E::custom("integer outside the JCS safe range"));
                }
                Ok(StrictValue(Value::Number(Number::from(value))))
            }
            fn visit_f64<E: de::Error>(self, value: f64) -> std::result::Result<Self::Value, E> {
                // Exponent/decimal syntax must not bypass the safe-integer rule;
                // canonical output must remain valid when parsed again.
                if value.fract() == 0.0 && value.abs() > MAX_SAFE_INTEGER as f64 {
                    return Err(E::custom("integer outside the JCS safe range"));
                }
                Number::from_f64(value)
                    .map(|n| StrictValue(Value::Number(n)))
                    .ok_or_else(|| E::custom("non-finite JSON number"))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<Self::Value, E> {
                self.visit_string(value.to_owned())
            }
            fn visit_string<E: de::Error>(
                self,
                value: String,
            ) -> std::result::Result<Self::Value, E> {
                if value.contains('\0') {
                    return Err(E::custom("NUL is not allowed"));
                }
                Ok(StrictValue(Value::String(value)))
            }
            fn visit_seq<A: de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(StrictValue(value)) = seq.next_element()? {
                    values.push(value);
                }
                Ok(StrictValue(Value::Array(values)))
            }
            fn visit_map<A: de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if key.contains('\0') || values.contains_key(&key) {
                        return Err(de::Error::custom("duplicate JSON key or NUL"));
                    }
                    let StrictValue(value) = map.next_value()?;
                    values.insert(key, value);
                }
                Ok(StrictValue(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}
