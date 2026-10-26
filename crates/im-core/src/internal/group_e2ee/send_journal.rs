//! Frozen application ciphertext in the existing owner/device group journal.
use super::v2_product::V2PreparedApplicationSend;
use crate::internal::group_runtime::journal;
use anp::group_e2ee::V2GroupSendResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreparedSend {
    schema_version: u32,
    request_binding: String,
    pub(crate) encrypted: V2PreparedApplicationSend,
    pub(crate) attachment_projection: Option<Value>,
}

pub(crate) struct SendJournal {
    key: String,
    binding: String,
    scope: String,
}

fn invalid() -> crate::ImError {
    crate::ImError::LocalStateUnavailable {
        detail: "P6 application send journal is invalid or conflicts with the request".into(),
    }
}

fn scope(client: &crate::core::ImClient) -> crate::ImResult<String> {
    let device = crate::internal::identity_device_state::active_authorization_for_client(client)?;
    Ok(format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&serde_json::json!([
                client.current_identity().id.as_str(),
                client.did().as_str(),
                device.protocol_device_id.as_str(),
                device.signing_key_id,
                device.e2ee_key_id,
                device.auth_generation
            ]))
            .map_err(|_| invalid())?
        )
    ))
}

impl SendJournal {
    pub(crate) fn new(
        client: &crate::core::ImClient,
        request: &crate::messages::SendMessageRequest,
    ) -> crate::ImResult<Self> {
        let operation = request
            .delivery
            .idempotency_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(invalid)?;
        let scope = scope(client)?;
        let binding = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(
                    scope.as_str(),
                    &request.target,
                    &request.body,
                    &request.security,
                    &request.client_message_id,
                    operation,
                    &request.delegated_signing,
                ))
                .map_err(|_| invalid())?
            )
        );
        Ok(Self {
            key: format!("p6-application-send:{operation}"),
            binding,
            scope,
        })
    }

    fn check_scope(&self, client: &crate::core::ImClient) -> crate::ImResult<()> {
        if scope(client)? != self.scope {
            return Err(invalid());
        }
        Ok(())
    }

    fn decode(&self, value: Value) -> crate::ImResult<PreparedSend> {
        let saved: PreparedSend = serde_json::from_value(value).map_err(|_| invalid())?;
        if saved.schema_version != 1 || saved.request_binding != self.binding {
            return Err(invalid());
        }
        saved.encrypted.meta.validate().map_err(|_| invalid())?;
        saved.encrypted.cipher.validate().map_err(|_| invalid())?;
        if saved.encrypted.meta.target.did != saved.encrypted.cipher.group_state_ref.group_did {
            return Err(invalid());
        }
        Ok(saved)
    }

    pub(crate) fn load(
        &self,
        client: &crate::core::ImClient,
    ) -> crate::ImResult<Option<PreparedSend>> {
        self.check_scope(client)?;
        journal::load(client, &self.key)?
            .map(|v| self.decode(v))
            .transpose()
    }

    /// The synchronous producer may encrypt, but must never perform network IO.
    /// Its result is exposed only after the existing journal transaction commits.
    pub(crate) fn prepare(
        &self,
        client: &crate::core::ImClient,
        produce: impl FnOnce() -> crate::ImResult<(V2PreparedApplicationSend, Option<Value>)>,
    ) -> crate::ImResult<PreparedSend> {
        self.check_scope(client)?;
        let saved = journal::freeze_with(client, &self.key, &self.binding, || {
            let (encrypted, attachment_projection) = produce()?;
            serde_json::to_value(PreparedSend {
                schema_version: 1,
                request_binding: self.binding.clone(),
                encrypted,
                attachment_projection,
            })
            .map_err(|_| invalid())
        })?;
        self.check_scope(client)?;
        self.decode(saved)
    }

    pub(crate) fn accepted(
        &self,
        client: &crate::core::ImClient,
        prepared: &PreparedSend,
    ) -> crate::ImResult<Option<V2GroupSendResult>> {
        self.check_scope(client)?;
        journal::load(client, &format!("{}:accepted", self.key))?
            .map(|value| {
                let binding = value.get("request_binding").and_then(Value::as_str);
                if binding != Some(self.binding.as_str()) {
                    return Err(invalid());
                }
                let result: V2GroupSendResult =
                    serde_json::from_value(value["result"].clone()).map_err(|_| invalid())?;
                validate_receipt(&prepared.encrypted, &result)?;
                Ok(result)
            })
            .transpose()
    }

    pub(crate) fn record_accepted(
        &self,
        client: &crate::core::ImClient,
        prepared: &PreparedSend,
        result: &V2GroupSendResult,
    ) -> crate::ImResult<()> {
        self.check_scope(client)?;
        validate_receipt(&prepared.encrypted, result)?;
        journal::freeze(
            client,
            &format!("{}:accepted", self.key),
            &self.binding,
            serde_json::json!({"request_binding": self.binding, "result": result}),
        )?;
        Ok(())
    }
}

fn validate_receipt(
    prepared: &V2PreparedApplicationSend,
    result: &V2GroupSendResult,
) -> crate::ImResult<()> {
    result.validate().map_err(|_| invalid())?;
    if result.group_did != prepared.cipher.group_state_ref.group_did
        || result.message_id != prepared.meta.message_id
        || result.operation_id != prepared.meta.operation_id
        || result.group_state_version != prepared.cipher.group_state_ref.group_state_version
        || result.epoch != prepared.cipher.epoch
    {
        return Err(invalid());
    }
    Ok(())
}
