use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub struct RuntimeIdentityRecord {
    pub identity_name: String,
    pub did: String,
    pub did_document: Option<Value>,
    pub key1_private_pem: String,
}
