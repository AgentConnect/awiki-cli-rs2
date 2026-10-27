//! New attempts are allowed only when the Host proves an irreversible version
//! advance. Every old ciphertext and its rejection remain immutable; uncertain
//! submissions never advance to a new attempt.
use super::*;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpiredSubmission {
    schema_version: u8,
    reason: String,
    current_value: String,
    group_did: String,
    operation_id: String,
    group_state_version: String,
    epoch: String,
    crypto_group_id_b64u: String,
}

impl ExpiredSubmission {
    fn validate(&self, prepared: &PreparedSend) -> crate::ImResult<()> {
        let cipher = &prepared.encrypted.cipher;
        if self.schema_version != 1
            || self.group_did != cipher.group_state_ref.group_did
            || self.operation_id != prepared.encrypted.meta.operation_id
            || self.group_state_version != cipher.group_state_ref.group_state_version
            || self.epoch != cipher.epoch
            || self.crypto_group_id_b64u != cipher.crypto_group_id_b64u
        {
            return Err(invalid());
        }
        let prior = match self.reason.as_str() {
            "state_version_advanced" => &self.group_state_version,
            "epoch_advanced" => &self.epoch,
            _ => return Err(invalid()),
        };
        if !crate::internal::local_state::sync_v2::compare_decimal(&self.current_value, prior)?
            .is_gt()
        {
            return Err(invalid());
        }
        Ok(())
    }
}

fn prepared_digest(prepared: &PreparedSend) -> crate::ImResult<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(prepared).map_err(|_| invalid())?)
    ))
}

impl SendJournal {
    pub(super) fn attempt_key(&self, attempt: u32) -> String {
        if attempt == 0 {
            self.key.clone()
        } else {
            format!("{}:attempt:{attempt}", self.key)
        }
    }

    pub(super) fn operation_for_attempt(&self, attempt: u32) -> String {
        if attempt == 0 {
            return self.operation.clone();
        }
        format!(
            "p6-retry-v1:{:x}",
            Sha256::digest(
                serde_json::json!([self.scope, self.operation, attempt])
                    .to_string()
                    .as_bytes()
            )
        )
    }

    pub(crate) fn retry_operation(&self, previous: &PreparedSend) -> crate::ImResult<String> {
        let attempt = previous
            .attempt
            .checked_add(1)
            .filter(|attempt| *attempt < MAX_SEND_ATTEMPTS)
            .ok_or_else(invalid)?;
        Ok(self.operation_for_attempt(attempt))
    }

    pub(crate) fn record_expired(
        &self,
        client: &crate::ImClient,
        prepared: &PreparedSend,
        error: &crate::ImError,
    ) -> crate::ImResult<bool> {
        self.check_scope(client)?;
        let crate::ImError::Service {
            code: Some(code),
            data: Some(data),
            ..
        } = error
        else {
            return Ok(false);
        };
        if !matches!(
            code.as_str(),
            "group.state_version_conflict" | "group.e2ee.epoch_conflict"
        ) {
            return Ok(false);
        }
        let Some(raw) = data.get("expired_submission") else {
            return Ok(false);
        };
        let version = raw
            .get("schema_version")
            .and_then(Value::as_u64)
            .ok_or_else(invalid)?;
        if version != 1 {
            return Ok(false);
        }
        let proof: ExpiredSubmission =
            serde_json::from_value(raw.clone()).map_err(|_| invalid())?;
        proof.validate(prepared)?;
        if (code == "group.state_version_conflict") != (proof.reason == "state_version_advanced")
            || self.accepted(client, prepared)?.is_some()
        {
            return Err(invalid());
        }
        journal::freeze(
            client,
            &format!("{}:expired", self.attempt_key(prepared.attempt)),
            &self.binding,
            serde_json::json!({"request_binding":self.binding,"prepared_digest":prepared_digest(prepared)?,"proof":proof}),
        )?;
        Ok(true)
    }

    pub(crate) fn is_expired(
        &self,
        client: &crate::ImClient,
        prepared: &PreparedSend,
    ) -> crate::ImResult<bool> {
        self.check_scope(client)?;
        let Some(record) = journal::load(
            client,
            &format!("{}:expired", self.attempt_key(prepared.attempt)),
        )?
        else {
            return Ok(false);
        };
        if record["request_binding"].as_str() != Some(self.binding.as_str())
            || record["prepared_digest"].as_str() != Some(prepared_digest(prepared)?.as_str())
            || self.accepted(client, prepared)?.is_some()
        {
            return Err(invalid());
        }
        let proof: ExpiredSubmission =
            serde_json::from_value(record["proof"].clone()).map_err(|_| invalid())?;
        proof.validate(prepared)?;
        Ok(true)
    }

    pub(crate) fn prepare_retry(
        &self,
        client: &crate::ImClient,
        previous: &PreparedSend,
        produce: impl FnOnce() -> crate::ImResult<(V2PreparedApplicationSend, Option<Value>)>,
    ) -> crate::ImResult<PreparedSend> {
        if !self.is_expired(client, previous)? {
            return Err(invalid());
        }
        let attempt = previous
            .attempt
            .checked_add(1)
            .filter(|attempt| *attempt < MAX_SEND_ATTEMPTS)
            .ok_or_else(invalid)?;
        let value =
            journal::freeze_with(client, &self.attempt_key(attempt), &self.binding, || {
                let (encrypted, attachment_projection) = produce()?;
                let value = serde_json::to_value(PreparedSend {
                    schema_version: 1,
                    request_binding: self.binding.clone(),
                    attempt,
                    encrypted,
                    attachment_projection,
                })
                .map_err(|_| invalid())?;
                self.decode(value.clone())?;
                Ok(value)
            })?;
        self.check_scope(client)?;
        self.decode(value)
    }
}
