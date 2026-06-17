use super::*;

static AUDIT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

impl DaemonState {
    pub fn store_runtime_token(&self, issued: &IssuedRuntimeToken) -> Result<()> {
        let connection = self.connection()?;
        let allowed_methods_json = serde_json::to_string(&issued.scope.allowed_methods)?;
        let allowed_recipients_json = match issued.scope.allowed_recipients.as_ref() {
            Some(recipients) => Some(serde_json::to_string(recipients)?),
            None => None,
        };
        let allowed_message_security_json = match issued.scope.allowed_message_security.as_ref() {
            Some(security_modes) => Some(serde_json::to_string(security_modes)?),
            None => None,
        };
        connection.execute(
            r#"
INSERT INTO runtime_rpc_tokens (
    token_id,
    token_secret_hash,
    agent_did,
    runtime_profile_id,
    run_id,
    allowed_methods_json,
    allowed_recipients_json,
    allowed_message_security_json,
    expires_at_ms,
    single_use,
    revoked_at_ms,
    used_at_ms,
    created_at_ms,
    expires_at,
    created_at
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, NULL, ?11, ?12, ?13)
"#,
            rusqlite::params![
                issued.token_id,
                issued.token.secret_hash(),
                issued.scope.agent_did,
                issued.scope.runtime_profile_id,
                issued.scope.run_id,
                allowed_methods_json,
                allowed_recipients_json,
                allowed_message_security_json,
                issued.scope.expires_at_ms,
                if issued.scope.single_use {
                    1_i64
                } else {
                    0_i64
                },
                current_time_millis()?,
                issued.scope.expires_at_ms.to_string(),
                current_time_millis()?.to_string(),
            ],
        )?;
        Ok(())
    }

    pub fn revoke_runtime_token(&self, token_id: &str) -> Result<()> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE runtime_rpc_tokens SET revoked_at_ms = ?1 WHERE token_id = ?2",
            rusqlite::params![current_time_millis()?, token_id],
        )?;
        Ok(())
    }

    pub fn authorize_runtime_rpc(
        &self,
        token: &RuntimeRpcToken,
        method: &RpcMethod,
        recipient: Option<&str>,
    ) -> Result<AuthorizedRuntimeContext> {
        self.authorize_runtime_rpc_with_message_policy(
            token,
            method,
            recipient.into_iter().collect::<Vec<_>>(),
            None,
        )
    }

    pub fn authorize_runtime_rpc_with_message_policy<'a>(
        &self,
        token: &RuntimeRpcToken,
        method: &RpcMethod,
        recipient_candidates: impl IntoIterator<Item = &'a str>,
        message_security: Option<&str>,
    ) -> Result<AuthorizedRuntimeContext> {
        self.authorize_runtime_rpc_internal(
            token,
            method,
            recipient_candidates,
            message_security,
            true,
        )
    }

    pub fn authorize_runtime_rpc_for_recipient_resolution(
        &self,
        token: &RuntimeRpcToken,
        method: &RpcMethod,
    ) -> Result<AuthorizedRuntimeContext> {
        self.authorize_runtime_rpc_internal(token, method, std::iter::empty::<&str>(), None, false)
    }

    fn authorize_runtime_rpc_internal<'a>(
        &self,
        token: &RuntimeRpcToken,
        method: &RpcMethod,
        recipient_candidates: impl IntoIterator<Item = &'a str>,
        message_security: Option<&str>,
        enforce_message_policy: bool,
    ) -> Result<AuthorizedRuntimeContext> {
        let connection = self.connection()?;
        let token_id = token.token_id();
        let record = load_runtime_token_record(&connection, &token_id)?;
        let audit_scope = record.scope_for_audit();
        let recipient_candidates = recipient_candidates
            .into_iter()
            .filter_map(|candidate| {
                let candidate = candidate.trim();
                (!candidate.is_empty()).then(|| candidate.to_string())
            })
            .collect::<Vec<_>>();
        let mut authorized = false;
        let mut reason = "authorized".to_string();

        let result = (|| {
            if record.token_secret_hash != token.secret_hash() {
                reason = "token_hash_mismatch".to_string();
                bail!("runtime RPC token rejected");
            }
            let now = current_time_millis()?;
            if record.scope.expires_at_ms <= now {
                reason = "token_expired".to_string();
                bail!("runtime RPC token expired");
            }
            if record.revoked_at_ms.is_some() {
                reason = "token_revoked".to_string();
                bail!("runtime RPC token revoked");
            }
            if record.single_use && record.used_at_ms.is_some() {
                reason = "token_already_used".to_string();
                bail!("runtime RPC token already used");
            }
            if !record.scope.allows_method(method) {
                reason = "method_not_allowed".to_string();
                bail!("runtime RPC method not allowed");
            }
            if *method == RpcMethod::MsgSend && enforce_message_policy {
                if !record
                    .scope
                    .allows_recipient_candidates(recipient_candidates.iter().map(String::as_str))
                {
                    reason = "recipient_not_allowed".to_string();
                    bail!("runtime RPC recipient not allowed");
                }
                if !record.scope.allows_message_security(message_security) {
                    reason = "message_security_not_allowed".to_string();
                    bail!("runtime RPC message security not allowed");
                }
            }
            authorized = true;
            Ok(AuthorizedRuntimeContext {
                token_id: token_id.clone(),
                agent_did: record.scope.agent_did.clone(),
                runtime_profile_id: record.scope.runtime_profile_id.clone(),
                run_id: record.scope.run_id.clone(),
                method: method.clone(),
            })
        })();

        self.insert_audit_event(
            &token_id,
            audit_scope,
            method,
            authorized,
            &reason,
            &recipient_candidates,
            message_security,
            enforce_message_policy,
        )?;

        let context = result?;
        if record.single_use {
            connection.execute(
                "UPDATE runtime_rpc_tokens SET used_at_ms = ?1 WHERE token_id = ?2",
                rusqlite::params![current_time_millis()?, token_id],
            )?;
        }
        Ok(context)
    }

    fn insert_audit_event(
        &self,
        token_id: &str,
        scope: RuntimeTokenAuditScope,
        method: &RpcMethod,
        authorized: bool,
        reason: &str,
        recipient_candidates: &[String],
        message_security: Option<&str>,
        enforce_message_policy: bool,
    ) -> Result<()> {
        let connection = self.connection()?;
        let now = current_time_millis()?;
        let sequence = AUDIT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let audit_id = format!("audit_{now}_{sequence}_{token_id}");
        let detail_json = serde_json::json!({
            "method": method.as_str(),
            "method_level": method.level(),
            "authorized": authorized,
            "reason": reason,
            "recipient_candidates": recipient_candidates,
            "message_security": message_security,
            "message_policy_enforced": enforce_message_policy,
        })
        .to_string();
        connection.execute(
            r#"
INSERT INTO audit_log (
    audit_id,
    event_type,
    agent_did,
    runtime_profile_id,
    run_id,
    token_id,
    detail_json,
    created_at_ms
) VALUES (?1, 'runtime_rpc.authorize', ?2, ?3, ?4, ?5, ?6, ?7)
"#,
            rusqlite::params![
                audit_id,
                scope.agent_did,
                scope.runtime_profile_id,
                scope.run_id,
                token_id,
                detail_json,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn insert_audit_event_json(
        &self,
        event_type: &str,
        agent_did: Option<&str>,
        runtime_profile_id: Option<&str>,
        run_id: Option<&str>,
        token_id: Option<&str>,
        detail: serde_json::Value,
    ) -> Result<()> {
        let connection = self.connection()?;
        let now = current_time_millis()?;
        let sequence = AUDIT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let audit_id = format!(
            "audit_{now}_{sequence}_{}",
            token_id.unwrap_or("agent_management")
        );
        connection.execute(
            r#"
INSERT INTO audit_log (
    audit_id,
    event_type,
    agent_did,
    runtime_profile_id,
    run_id,
    token_id,
    detail_json,
    created_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
"#,
            rusqlite::params![
                audit_id,
                event_type,
                agent_did,
                runtime_profile_id,
                run_id,
                token_id,
                detail.to_string(),
                now,
            ],
        )?;
        Ok(())
    }

    pub fn audit_event_exists(
        &self,
        event_type: &str,
        agent_did: Option<&str>,
        detail_contains: Option<&str>,
    ) -> Result<bool> {
        if event_type.trim().is_empty() {
            bail!("event_type must not be empty");
        }
        let connection = self.connection()?;
        let mut sql = "SELECT COUNT(*) FROM audit_log WHERE event_type = ?1".to_string();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(event_type.to_string())];
        if let Some(agent_did) = agent_did {
            sql.push_str(" AND agent_did = ?");
            params.push(Box::new(agent_did.to_string()));
        }
        if let Some(detail_contains) = detail_contains {
            sql.push_str(" AND COALESCE(detail_json, '') LIKE ?");
            params.push(Box::new(format!("%{detail_contains}%")));
        }
        let count: i64 = connection.query_row(
            &sql,
            rusqlite::params_from_iter(params.iter().map(|value| value.as_ref())),
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizedRuntimeContext {
    pub token_id: String,
    pub agent_did: String,
    pub runtime_profile_id: String,
    pub run_id: String,
    pub method: RpcMethod,
}

#[derive(Debug, Clone)]
struct RuntimeTokenRecord {
    token_secret_hash: String,
    scope: RuntimeTokenScope,
    single_use: bool,
    revoked_at_ms: Option<i64>,
    used_at_ms: Option<i64>,
}

#[derive(Debug, Clone)]
struct RuntimeTokenAuditScope {
    agent_did: String,
    runtime_profile_id: String,
    run_id: String,
}

impl RuntimeTokenRecord {
    fn scope_for_audit(&self) -> RuntimeTokenAuditScope {
        RuntimeTokenAuditScope {
            agent_did: self.scope.agent_did.clone(),
            runtime_profile_id: self.scope.runtime_profile_id.clone(),
            run_id: self.scope.run_id.clone(),
        }
    }
}

fn load_runtime_token_record(
    connection: &Connection,
    token_id: &str,
) -> Result<RuntimeTokenRecord> {
    let record = connection.query_row(
        r#"
SELECT
    token_secret_hash,
    agent_did,
    runtime_profile_id,
    run_id,
    allowed_methods_json,
    allowed_recipients_json,
    allowed_message_security_json,
    expires_at_ms,
    single_use,
    revoked_at_ms,
    used_at_ms
FROM runtime_rpc_tokens
WHERE token_id = ?1
"#,
        [token_id],
        |row| {
            let allowed_methods_json: String = row.get(4)?;
            let allowed_recipients_json: Option<String> = row.get(5)?;
            let allowed_message_security_json: Option<String> = row.get(6)?;
            let allowed_methods: Vec<RpcMethod> = serde_json::from_str(&allowed_methods_json)
                .map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        allowed_methods_json.len(),
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?;
            let allowed_recipients = allowed_recipients_json
                .as_ref()
                .map(|json| serde_json::from_str(json))
                .transpose()
                .map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        allowed_recipients_json.as_deref().unwrap_or_default().len(),
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?;
            let allowed_message_security = allowed_message_security_json
                .as_ref()
                .map(|json| serde_json::from_str(json))
                .transpose()
                .map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        allowed_message_security_json
                            .as_deref()
                            .unwrap_or_default()
                            .len(),
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?;
            let single_use = row.get::<_, i64>(8)? != 0;
            Ok(RuntimeTokenRecord {
                token_secret_hash: row.get(0)?,
                scope: RuntimeTokenScope {
                    agent_did: row.get(1)?,
                    runtime_profile_id: row.get(2)?,
                    run_id: row.get(3)?,
                    allowed_methods,
                    allowed_recipients,
                    allowed_message_security,
                    expires_at_ms: row.get(7)?,
                    single_use,
                },
                single_use,
                revoked_at_ms: row.get(9)?,
                used_at_ms: row.get(10)?,
            })
        },
    )?;
    Ok(record)
}
