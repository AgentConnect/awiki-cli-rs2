use serde::Serialize;
use serde_json::Value;

use crate::cli_output::{IdentityMeta, Meta, SuccessEnvelope};

pub fn success_envelope_for_sdk_value<T: Serialize>(
    command: impl Into<String>,
    data: &T,
    meta: Meta,
    summary: impl Into<String>,
    warnings: Vec<String>,
) -> Result<SuccessEnvelope, serde_json::Error> {
    Ok(SuccessEnvelope {
        ok: true,
        command: command.into(),
        data: serde_json::to_value(data)?,
        warnings,
        summary: summary.into(),
        notice: None,
        meta,
    })
}

pub fn identity_meta_from_sdk(identity: &im_core::IdentitySummary) -> IdentityMeta {
    IdentityMeta {
        name: identity.local_alias.clone().unwrap_or_default(),
        did: identity.did.as_str().to_string(),
    }
}

pub fn null_success_data() -> Value {
    Value::Null
}
