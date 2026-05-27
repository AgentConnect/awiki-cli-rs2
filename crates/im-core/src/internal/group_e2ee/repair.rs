use anp::group_e2ee::operations::{
    FinalizeCommitInput, ProcessNoticeInput, ProcessWelcomeInput, StatusInput, StatusOutput,
};
use anp::group_e2ee::GroupStateRef;
use serde_json::{Map, Value};

use crate::internal::auth::session::SessionProvider;
use crate::internal::message_runtime::group::{load_credentials, GroupTextCredentials};
use crate::internal::transport::AuthenticatedRpcTransport;

use super::provider::GroupMlsProvider;
use super::status::{group_status_from_parts, GroupE2eeStatusResult};
use super::DEFAULT_GROUP_MLS_DEVICE_ID;

pub(crate) struct GroupE2eeRepairRuntime<'a, P, T, M> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
    mls_provider: M,
}

#[derive(Debug, Clone)]
pub(crate) struct GroupE2eeRepairInput {
    pub(crate) group: crate::ids::GroupRef,
    pub(crate) credentials: Option<GroupTextCredentials>,
    pub(crate) notice_limit: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GroupE2eeRepairResult {
    pub(crate) group: crate::ids::GroupRef,
    pub(crate) state: crate::secure::GroupSecureState,
    pub(crate) repaired: bool,
    pub(crate) problem: Option<crate::secure::SecureProblem>,
    pub(crate) warnings: Vec<String>,
}

impl<'a, P, T, M> GroupE2eeRepairRuntime<'a, P, T, M>
where
    P: SessionProvider,
    T: AuthenticatedRpcTransport,
    M: GroupMlsProvider,
{
    pub(crate) fn new(
        client: &'a crate::core::ImClient,
        session_provider: P,
        transport: T,
        mls_provider: M,
    ) -> Self {
        Self {
            client,
            session_provider,
            transport,
            mls_provider,
        }
    }

    pub(crate) fn repair(
        mut self,
        input: GroupE2eeRepairInput,
    ) -> crate::ImResult<GroupE2eeRepairResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)?;
        let group_did = require_group(input.group.as_str())?.to_owned();
        let credentials = match input.credentials {
            Some(credentials) => credentials,
            None => load_credentials(self.client)?,
        };
        let mut warnings = Vec::new();
        let mut repaired = false;

        let service_head = match self.service_head(&credentials, &group_did) {
            Ok(head) => Some(head),
            Err(err) => {
                warnings.push(format!(
                    "group E2EE service head unavailable during repair: {err}"
                ));
                None
            }
        };
        if let Some(service_head) = service_head.as_ref() {
            let (finalized_count, finalize_warnings) =
                self.finalize_accepted_pending_commits(&group_did, service_head);
            repaired |= finalized_count > 0;
            warnings.extend(finalize_warnings);
        }

        let pending =
            self.pull_notices(&credentials, &group_did, input.notice_limit, false, &[])?;
        let notices = notice_objects(pending.get("notices"));
        let mut delivered_notice_ids = Vec::new();
        for notice in notices {
            let notice_type = string_value(notice.get("notice_type"));
            if !is_welcome_notice_type(&notice_type) && notice_type != "commit-delivery" {
                continue;
            }
            let target_group_did =
                default_string(&string_value(notice.get("group_did")), &group_did);
            if target_group_did.trim().is_empty() {
                warnings.push(format!(
                    "group E2EE repair skipped {notice_type} notice without group_did"
                ));
                continue;
            }
            let mut recipient =
                first_non_empty_string(&[notice.get("recipient_did"), notice.get("member_did")]);
            if is_welcome_notice_type(&notice_type) && recipient.is_empty() {
                recipient = string_value(notice.get("subject_did"));
            }
            if !recipient.is_empty() && recipient != self.client.did().as_str() {
                warnings.push(format!(
                    "group E2EE repair skipped notice for different recipient {recipient}"
                ));
                continue;
            }

            let device_id = default_string(
                &string_value(notice.get("device_id")),
                device_id_for_client(self.client).as_str(),
            );
            let processed = if notice_type == "commit-delivery" {
                self.process_commit_notice(&target_group_did, &device_id, &notice, &mut warnings)
            } else {
                self.process_welcome_notice(&target_group_did, &device_id, &notice, &mut warnings)
            };
            if processed {
                repaired = true;
                if let Some(notice_id) = notice_id(&notice) {
                    delivered_notice_ids.push(notice_id);
                }
            }
        }

        if !delivered_notice_ids.is_empty() {
            if let Err(err) = self.pull_notices(
                &credentials,
                &group_did,
                delivered_notice_ids.len() as i64,
                true,
                &delivered_notice_ids,
            ) {
                warnings.push(format!(
                    "group E2EE repair processed notices but failed to mark delivered: {err}"
                ));
            }
        }

        let status = self.status_after_repair(
            input.group.clone(),
            &credentials,
            &group_did,
            input.notice_limit,
        )?;
        warnings.extend(status.warnings.clone());
        Ok(repair_result_from_status(
            status,
            repaired,
            compact_warnings(warnings),
        ))
    }

    fn status_after_repair(
        &mut self,
        group: crate::ids::GroupRef,
        credentials: &GroupTextCredentials,
        group_did: &str,
        notice_limit: i64,
    ) -> crate::ImResult<GroupE2eeStatusResult> {
        let mut warnings = Vec::new();
        let (local_status, local_error) = match self.mls_provider.status(StatusInput {
            request_id: format!(
                "group-e2ee-repair-final-status-{}",
                crate::internal::wire::common::generate_operation_id()
            ),
            device_id: device_id_for_client(self.client),
            agent_did: Some(self.client.did().as_str().to_owned()),
            group_did: Some(group_did.to_owned()),
        }) {
            Ok(status) => (Some(status), None),
            Err(err) => {
                warnings.push("group E2EE local MLS status is unavailable".to_owned());
                (None, Some(err.to_string()))
            }
        };
        let service_head = match self.service_head(credentials, group_did) {
            Ok(head) => Some(head),
            Err(err) => {
                warnings.push(format!("group E2EE service head unavailable: {err}"));
                None
            }
        };
        let pending_notice_count =
            match self.pull_notices(credentials, group_did, notice_limit, false, &[]) {
                Ok(value) => pending_notice_count(&value),
                Err(err) => {
                    warnings.push(format!(
                        "group E2EE pending notice status unavailable: {err}"
                    ));
                    0
                }
            };
        Ok(group_status_from_parts(
            group,
            local_status.as_ref(),
            service_head.as_ref(),
            pending_notice_count,
            local_error.as_deref(),
            warnings,
        ))
    }

    fn service_head(
        &mut self,
        credentials: &GroupTextCredentials,
        group_did: &str,
    ) -> crate::ImResult<Value> {
        let params = super::wire::build_group_e2ee_head_rpc_params(
            credentials,
            self.client.did().as_str(),
            group_did,
        )?;
        self.transport.authenticated_rpc(
            crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
            "group.e2ee.head",
            params,
        )
    }

    fn pull_notices(
        &mut self,
        credentials: &GroupTextCredentials,
        group_did: &str,
        limit: i64,
        mark_delivered: bool,
        notice_ids: &[String],
    ) -> crate::ImResult<Value> {
        let params = super::wire::build_group_e2ee_notice_rpc_params(
            credentials,
            self.client.did().as_str(),
            group_did,
            limit,
            mark_delivered,
            notice_ids,
        )?;
        self.transport.authenticated_rpc(
            crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
            "group.e2ee.notice",
            params,
        )
    }

    fn finalize_accepted_pending_commits(
        &self,
        group_did: &str,
        service_head: &Value,
    ) -> (u32, Vec<String>) {
        let status = match self.mls_provider.status(StatusInput {
            request_id: format!(
                "group-e2ee-repair-pending-status-{}",
                crate::internal::wire::common::generate_operation_id()
            ),
            device_id: device_id_for_client(self.client),
            agent_did: Some(self.client.did().as_str().to_owned()),
            group_did: Some(group_did.to_owned()),
        }) {
            Ok(status) => status,
            Err(err) => {
                return (
                    0,
                    vec![format!(
                        "group E2EE pending commit status unavailable during repair: {err}"
                    )],
                )
            }
        };
        let mut finalized = 0;
        let mut warnings = Vec::new();
        for pending in &status.pending_commits {
            if !pending_commit_accepted_by_service(pending.to_epoch.as_str(), service_head) {
                warnings.push(
                    "group E2EE pending commit retained: service head has not accepted its target state"
                        .to_string(),
                );
                continue;
            }
            match self.mls_provider.finalize_commit(FinalizeCommitInput {
                pending_commit_id: pending.pending_commit_id.clone(),
                request_id: format!(
                    "group-e2ee-repair-finalize-{}",
                    crate::internal::wire::common::generate_operation_id()
                ),
            }) {
                Ok(_) => finalized += 1,
                Err(err) => warnings.push(format!(
                    "group E2EE pending commit matched service head but local finalize failed: {err}"
                )),
            }
        }
        (finalized, warnings)
    }

    fn process_commit_notice(
        &self,
        group_did: &str,
        device_id: &str,
        notice: &Map<String, Value>,
        warnings: &mut Vec<String>,
    ) -> bool {
        let commit_b64u = string_value(notice.get("commit_b64u"));
        if commit_b64u.is_empty() {
            warnings.push("group E2EE repair skipped commit notice missing commit_b64u".to_owned());
            return false;
        }
        let from_epoch = string_value(notice.get("from_epoch"));
        if from_epoch.is_empty() {
            warnings.push("group E2EE repair skipped commit notice missing from_epoch".to_owned());
            return false;
        }
        match self.mls_provider.process_notice(ProcessNoticeInput {
            recipient_did: self.client.did().as_str().to_owned(),
            device_id: device_id.to_owned(),
            group_did: group_did.to_owned(),
            commit_b64u,
            from_epoch,
            subject_did: optional_string(notice.get("subject_did")),
            subject_status: optional_string(notice.get("subject_status")),
            request_id: format!(
                "group-e2ee-repair-notice-{}",
                crate::internal::wire::common::generate_operation_id()
            ),
        }) {
            Ok(output) => {
                self.persist_group_e2ee_summary(
                    group_did,
                    Some(output.epoch.as_str()),
                    optional_string(notice.get("group_state_version")).as_deref(),
                    Some(output.crypto_group_id_b64u.as_str()),
                );
                true
            }
            Err(_err)
                if commit_notice_already_applied(
                    &self.mls_provider,
                    self.client,
                    group_did,
                    device_id,
                    notice,
                ) =>
            {
                warnings.push(
                    "group E2EE repair treated duplicate/already-applied commit notice as delivered"
                        .to_owned(),
                );
                true
            }
            Err(err) => {
                warnings.push(format!(
                    "group E2EE repair commit processing failed on device {device_id}: {err}"
                ));
                false
            }
        }
    }

    fn process_welcome_notice(
        &self,
        group_did: &str,
        device_id: &str,
        notice: &Map<String, Value>,
        warnings: &mut Vec<String>,
    ) -> bool {
        let welcome_b64u = string_value(notice.get("welcome_b64u"));
        if welcome_b64u.is_empty() {
            warnings
                .push("group E2EE repair skipped welcome notice missing welcome_b64u".to_owned());
            return false;
        }
        let ratchet_tree_b64u = string_value(notice.get("ratchet_tree_b64u"));
        if ratchet_tree_b64u.is_empty() {
            warnings.push(
                "group E2EE repair skipped welcome notice missing ratchet_tree_b64u".to_owned(),
            );
            return false;
        }
        let group_state_ref = match group_state_ref_from_notice(group_did, notice) {
            Some(reference) => reference,
            None => {
                warnings.push(
                    "group E2EE repair skipped welcome notice missing group_state_ref".to_owned(),
                );
                return false;
            }
        };
        let crypto_group_id_b64u = string_value(notice.get("crypto_group_id_b64u"));
        if crypto_group_id_b64u.is_empty() {
            warnings.push(
                "group E2EE repair skipped welcome notice missing crypto_group_id_b64u".to_owned(),
            );
            return false;
        }
        let epoch = first_non_empty_string(&[notice.get("to_epoch"), notice.get("epoch")]);
        if epoch.is_empty() {
            warnings
                .push("group E2EE repair skipped welcome notice missing target epoch".to_owned());
            return false;
        }
        match self.mls_provider.process_welcome(ProcessWelcomeInput {
            agent_did: self.client.did().as_str().to_owned(),
            device_id: device_id.to_owned(),
            group_did: group_did.to_owned(),
            welcome_b64u,
            ratchet_tree_b64u,
            group_state_ref,
            crypto_group_id_b64u,
            epoch: epoch.clone(),
            request_id: format!(
                "group-e2ee-repair-welcome-{}",
                crate::internal::wire::common::generate_operation_id()
            ),
        }) {
            Ok(output) => {
                self.persist_group_e2ee_summary(
                    group_did,
                    Some(output.epoch.as_str()),
                    optional_string(notice.get("group_state_version")).as_deref(),
                    Some(output.crypto_group_id_b64u.as_str()),
                );
                true
            }
            Err(_err)
                if welcome_notice_already_available(
                    &self.mls_provider,
                    self.client,
                    group_did,
                    device_id,
                    notice,
                ) =>
            {
                warnings.push(
                    "group E2EE repair treated duplicate/already-applied welcome notice as delivered"
                        .to_owned(),
                );
                true
            }
            Err(err) => {
                warnings.push(format!(
                    "group E2EE repair welcome processing failed: {err}"
                ));
                false
            }
        }
    }

    #[cfg(feature = "sqlite")]
    fn persist_group_e2ee_summary(
        &self,
        group_did: &str,
        epoch: Option<&str>,
        group_state_version: Option<&str>,
        crypto_group_id_b64u: Option<&str>,
    ) {
        let Ok(connection) = crate::internal::local_state::open_writable(
            &self.client.core_inner().sdk_paths().local_state.sqlite_path,
        ) else {
            return;
        };
        let mut group_e2ee = Map::new();
        insert_string(&mut group_e2ee, "epoch", epoch);
        insert_string(&mut group_e2ee, "group_state_version", group_state_version);
        insert_string(
            &mut group_e2ee,
            "crypto_group_id_b64u",
            crypto_group_id_b64u,
        );
        insert_string(
            &mut group_e2ee,
            "updated_at",
            Some(crate::internal::wire::common::now_rfc3339().as_str()),
        );
        let mut metadata = Map::new();
        metadata.insert(
            "message_security_profile".to_owned(),
            Value::String(super::wire::GROUP_E2EE_SECURITY_PROFILE.to_owned()),
        );
        metadata.insert("group_e2ee".to_owned(), Value::Object(group_e2ee));
        if let Some(group_state_version) =
            group_state_version.filter(|value| !value.trim().is_empty())
        {
            metadata.insert(
                "group_state_version".to_owned(),
                Value::String(group_state_version.to_owned()),
            );
        }
        let _ = crate::internal::local_state::groups::upsert_group(
            &connection,
            crate::internal::local_state::groups::GroupRecord {
                owner_identity_id: self.client.current_identity().id.as_str().to_owned(),
                owner_did: self.client.did().as_str().to_owned(),
                group_id: group_did.to_owned(),
                group_did: group_did.to_owned(),
                membership_status: "active".to_owned(),
                metadata: Value::Object(metadata).to_string(),
                credential_name: self.client.current_identity().id.as_str().to_owned(),
                ..crate::internal::local_state::groups::GroupRecord::default()
            },
        );
    }

    #[cfg(not(feature = "sqlite"))]
    fn persist_group_e2ee_summary(
        &self,
        _group_did: &str,
        _epoch: Option<&str>,
        _group_state_version: Option<&str>,
        _crypto_group_id_b64u: Option<&str>,
    ) {
    }
}

fn repair_result_from_status(
    status: GroupE2eeStatusResult,
    repaired: bool,
    warnings: Vec<String>,
) -> GroupE2eeRepairResult {
    GroupE2eeRepairResult {
        group: status.group,
        state: status.state,
        repaired,
        problem: status.problem,
        warnings,
    }
}

fn pending_commit_accepted_by_service(to_epoch: &str, service_head: &Value) -> bool {
    let Some(to_epoch) = parse_i64(to_epoch) else {
        return false;
    };
    let service_epoch = first_i64(&[
        service_head.get("epoch"),
        service_head
            .get("group_state_ref")
            .and_then(|reference| reference.get("epoch")),
        service_head.get("to_epoch"),
    ]);
    matches!(service_epoch, Some(service_epoch) if service_epoch >= to_epoch)
}

fn commit_notice_already_applied<M: GroupMlsProvider>(
    provider: &M,
    client: &crate::core::ImClient,
    group_did: &str,
    device_id: &str,
    notice: &Map<String, Value>,
) -> bool {
    let Some(to_epoch) = first_i64(&[notice.get("to_epoch"), notice.get("epoch")]) else {
        return false;
    };
    let Ok(status) = provider.status(StatusInput {
        request_id: format!(
            "group-e2ee-repair-duplicate-{}",
            crate::internal::wire::common::generate_operation_id()
        ),
        device_id: device_id.to_owned(),
        agent_did: Some(client.did().as_str().to_owned()),
        group_did: Some(group_did.to_owned()),
    }) else {
        return false;
    };
    local_epoch(&status)
        .map(|local_epoch| local_epoch >= to_epoch)
        .unwrap_or(false)
}

fn welcome_notice_already_available<M: GroupMlsProvider>(
    provider: &M,
    client: &crate::core::ImClient,
    group_did: &str,
    device_id: &str,
    notice: &Map<String, Value>,
) -> bool {
    let Ok(status) = provider.status(StatusInput {
        request_id: format!(
            "group-e2ee-repair-welcome-duplicate-{}",
            crate::internal::wire::common::generate_operation_id()
        ),
        device_id: device_id.to_owned(),
        agent_did: Some(client.did().as_str().to_owned()),
        group_did: Some(group_did.to_owned()),
    }) else {
        return false;
    };
    if !status.status.trim().eq_ignore_ascii_case("active") {
        return false;
    }
    let Some(target_epoch) = first_i64(&[notice.get("to_epoch"), notice.get("epoch")]) else {
        return true;
    };
    local_epoch(&status)
        .map(|local_epoch| local_epoch >= target_epoch)
        .unwrap_or(false)
}

fn group_state_ref_from_notice(
    group_did: &str,
    notice: &Map<String, Value>,
) -> Option<GroupStateRef> {
    if let Some(reference) = notice
        .get("group_state_ref")
        .and_then(Value::as_object)
        .filter(|value| !value.is_empty())
    {
        return serde_json::from_value(Value::Object(reference.clone())).ok();
    }
    let version = first_non_empty_string(&[
        notice.get("group_state_version"),
        notice
            .get("group_receipt")
            .and_then(|receipt| receipt.get("group_state_version")),
    ]);
    if version.is_empty() {
        return None;
    }
    Some(GroupStateRef {
        group_did: group_did.to_owned(),
        group_state_version: version,
        policy_hash: optional_string(notice.get("policy_hash")),
    })
}

fn notice_objects(value: Option<&Value>) -> Vec<Map<String, Value>> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|value| value.as_object().cloned())
                .collect()
        })
        .unwrap_or_default()
}

fn pending_notice_count(value: &Value) -> u32 {
    for key in ["pending_count", "pending_notice_count", "count"] {
        if let Some(count) = first_i64(&[value.get(key)]) {
            return count.max(0) as u32;
        }
    }
    value
        .get("notices")
        .and_then(Value::as_array)
        .map(|items| items.len() as u32)
        .unwrap_or_default()
}

fn notice_id(notice: &Map<String, Value>) -> Option<String> {
    optional_string(notice.get("notice_id"))
}

fn is_welcome_notice_type(notice_type: &str) -> bool {
    matches!(
        notice_type.trim(),
        "welcome-delivery" | "recovery-welcome-delivery" | "update-welcome-delivery"
    )
}

fn local_epoch(status: &StatusOutput) -> Option<i64> {
    status
        .local_epoch
        .as_deref()
        .or(status.epoch.as_deref())
        .and_then(parse_i64)
}

fn first_non_empty_string(values: &[Option<&Value>]) -> String {
    values
        .iter()
        .map(|value| value.and_then(string_from_value).unwrap_or_default())
        .find(|value| !value.trim().is_empty())
        .unwrap_or_default()
}

fn first_i64(values: &[Option<&Value>]) -> Option<i64> {
    for value in values.iter().flatten() {
        match value {
            Value::Number(number) => {
                if let Some(value) = number.as_i64() {
                    return Some(value);
                }
                if let Some(value) = number.as_u64() {
                    return Some(value.min(i64::MAX as u64) as i64);
                }
            }
            Value::String(text) => {
                if let Some(value) = parse_i64(text) {
                    return Some(value);
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_i64(value: &str) -> Option<i64> {
    value.trim().parse::<i64>().ok()
}

fn optional_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(string_from_value)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn string_value(value: Option<&Value>) -> String {
    value.and_then(string_from_value).unwrap_or_default()
}

fn string_from_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn default_string(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_owned()
    } else {
        value.trim().to_owned()
    }
}

#[cfg(feature = "sqlite")]
fn insert_string(map: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        map.insert(key.to_owned(), Value::String(value.to_owned()));
    }
}

fn compact_warnings(warnings: Vec<String>) -> Vec<String> {
    let mut compact = Vec::new();
    for warning in warnings {
        if !warning.trim().is_empty() && !compact.contains(&warning) {
            compact.push(warning);
        }
    }
    compact
}

fn require_group(group_did: &str) -> crate::ImResult<&str> {
    let group_did = group_did.trim();
    if group_did.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("group".to_owned()),
            "group target is required",
        ));
    }
    Ok(group_did)
}

fn device_id_for_client(client: &crate::core::ImClient) -> String {
    client
        .current_identity()
        .device_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(DEFAULT_GROUP_MLS_DEVICE_ID)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fs;
    use std::path::PathBuf;
    use std::rc::Rc;

    use anp::group_e2ee::operations::{
        AbortCommitInput, AbortCommitOutput, AddMemberInput, CreateGroupInput, DecryptInput,
        DecryptOutput, EncryptInput, EncryptOutput, FinalizeCommitInput, FinalizeCommitOutput,
        GenerateKeyPackageInput, GroupKeyPackageOutput, LeaveGroupInput, PendingCommitStatus,
        PreparedMlsCommitOutput, ProcessNoticeInput, ProcessNoticeOutput, ProcessWelcomeInput,
        ProcessWelcomeOutput, RecoverMemberInput, RemoveMemberInput, StatusInput, StatusOutput,
        UpdateMemberInput,
    };
    use serde_json::json;

    use super::*;

    #[test]
    fn repair_processes_commit_notice_and_marks_delivered_without_public_raw_notice() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let provider = RecordingMlsProvider::active("1");
        let result = GroupE2eeRepairRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                responses: vec![
                    (
                        "group.e2ee.head".to_owned(),
                        json!({"epoch": "2", "actor_membership_status": "active"}),
                    ),
                    (
                        "group.e2ee.notice".to_owned(),
                        json!({
                            "notices": [{
                                "notice_id": "notice-1",
                                "notice_type": "commit-delivery",
                                "group_did": "did:example:groups:e2ee",
                                "commit_b64u": "secret-commit",
                                "from_epoch": "1",
                                "to_epoch": "2",
                                "subject_did": "did:example:bob",
                                "subject_status": "active"
                            }]
                        }),
                    ),
                    (
                        "group.e2ee.head".to_owned(),
                        json!({"epoch": "2", "actor_membership_status": "active"}),
                    ),
                    (
                        "group.e2ee.notice".to_owned(),
                        json!({"pending_count": 0, "notices": []}),
                    ),
                    ("group.e2ee.notice".to_owned(), json!({"delivered": true})),
                ],
            },
            provider.clone(),
        )
        .repair(GroupE2eeRepairInput {
            group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
            credentials: Some(fixture.credentials()),
            notice_limit: 50,
        })
        .unwrap();

        assert!(result.repaired);
        assert_eq!(result.state, crate::secure::GroupSecureState::Ready);
        assert!(!format!("{result:?}").contains("secret-commit"));
        assert_eq!(
            provider.processed_notices.borrow().as_slice(),
            ["did:example:groups:e2ee:1"]
        );
        let calls = calls.borrow();
        assert!(calls.iter().any(|call| {
            call.method == "group.e2ee.notice"
                && call.params["body"]["mark_delivered"] == Value::Bool(true)
                && call.params["body"]["notice_ids"][0] == "notice-1"
        }));
    }

    #[test]
    fn repair_finalizes_pending_commit_when_service_head_accepted_target_epoch() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let provider = RecordingMlsProvider::with_pending_commit("7", "8");
        let result = GroupE2eeRepairRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::new(RefCell::new(Vec::new())),
                responses: vec![
                    (
                        "group.e2ee.head".to_owned(),
                        json!({"epoch": "8", "actor_membership_status": "active"}),
                    ),
                    (
                        "group.e2ee.notice".to_owned(),
                        json!({"pending_count": 0, "notices": []}),
                    ),
                    (
                        "group.e2ee.head".to_owned(),
                        json!({"epoch": "8", "actor_membership_status": "active"}),
                    ),
                    (
                        "group.e2ee.notice".to_owned(),
                        json!({"pending_count": 0, "notices": []}),
                    ),
                ],
            },
            provider.clone(),
        )
        .repair(GroupE2eeRepairInput {
            group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
            credentials: Some(fixture.credentials()),
            notice_limit: 50,
        })
        .unwrap();

        assert!(result.repaired);
        assert_eq!(result.state, crate::secure::GroupSecureState::Ready);
        assert_eq!(provider.finalized.borrow().as_slice(), ["pc-test"]);
    }

    #[derive(Clone)]
    struct ReadySessionProvider;

    impl SessionProvider for ReadySessionProvider {
        fn ensure_session(
            &self,
            scope: crate::auth::AuthScope,
        ) -> crate::ImResult<crate::auth::SessionBundle> {
            assert_eq!(scope, crate::auth::AuthScope::GroupMessaging);
            Ok(crate::auth::SessionBundle {
                subject: crate::ids::Did::parse("did:example:alice")?,
                scope,
                expires_at: None,
                refreshed: false,
            })
        }

        fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
            unreachable!("group E2EE repair should not refresh through the session provider")
        }

        fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("group E2EE repair should not read auth status")
        }
    }

    #[derive(Clone)]
    struct RecordingTransport {
        calls: Rc<RefCell<Vec<RecordedCall>>>,
        responses: Vec<(String, Value)>,
    }

    impl AuthenticatedRpcTransport for RecordingTransport {
        fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            self.calls.borrow_mut().push(RecordedCall {
                endpoint: endpoint.to_owned(),
                method: method.to_owned(),
                params,
            });
            if let Some((index, _)) = self
                .responses
                .iter()
                .enumerate()
                .find(|(_, (candidate, _))| candidate == method)
            {
                return Ok(self.responses.remove(index).1);
            }
            Err(crate::ImError::Service {
                status_code: None,
                code: Some("missing_test_response".to_owned()),
                message: format!("missing response for {method}"),
            })
        }
    }

    struct RecordedCall {
        endpoint: String,
        method: String,
        params: Value,
    }

    #[derive(Clone)]
    struct RecordingMlsProvider {
        status: Rc<RefCell<StatusOutput>>,
        processed_notices: Rc<RefCell<Vec<String>>>,
        finalized: Rc<RefCell<Vec<String>>>,
    }

    impl RecordingMlsProvider {
        fn active(epoch: &str) -> Self {
            Self {
                status: Rc::new(RefCell::new(active_status(epoch, Vec::new()))),
                processed_notices: Rc::new(RefCell::new(Vec::new())),
                finalized: Rc::new(RefCell::new(Vec::new())),
            }
        }

        fn with_pending_commit(from_epoch: &str, to_epoch: &str) -> Self {
            Self {
                status: Rc::new(RefCell::new(active_status(
                    from_epoch,
                    vec![PendingCommitStatus {
                        pending_commit_id: "pc-test".to_owned(),
                        operation_id: "op-test".to_owned(),
                        agent_did: "did:example:alice".to_owned(),
                        device_id: DEFAULT_GROUP_MLS_DEVICE_ID.to_owned(),
                        group_did: "did:example:groups:e2ee".to_owned(),
                        subject_did: "did:example:bob".to_owned(),
                        subject_status: "active".to_owned(),
                        from_epoch: from_epoch.to_owned(),
                        to_epoch: to_epoch.to_owned(),
                        status: "pending".to_owned(),
                    }],
                ))),
                processed_notices: Rc::new(RefCell::new(Vec::new())),
                finalized: Rc::new(RefCell::new(Vec::new())),
            }
        }
    }

    impl GroupMlsProvider for RecordingMlsProvider {
        fn status(&self, _input: StatusInput) -> crate::ImResult<StatusOutput> {
            Ok(self.status.borrow().clone())
        }

        fn finalize_commit(
            &self,
            input: FinalizeCommitInput,
        ) -> crate::ImResult<FinalizeCommitOutput> {
            self.finalized
                .borrow_mut()
                .push(input.pending_commit_id.clone());
            let mut status = self.status.borrow_mut();
            status.epoch = Some("8".to_owned());
            status.local_epoch = Some("8".to_owned());
            status.pending_commits.clear();
            Ok(FinalizeCommitOutput {
                pending_commit_id: input.pending_commit_id,
                operation_id: "op-test".to_owned(),
                group_did: "did:example:groups:e2ee".to_owned(),
                crypto_group_id_b64u: "crypto".to_owned(),
                status: "finalized".to_owned(),
                from_epoch: "7".to_owned(),
                epoch: "8".to_owned(),
                local_epoch: "8".to_owned(),
                subject_did: "did:example:bob".to_owned(),
                subject_status: "active".to_owned(),
                epoch_authenticator: None,
            })
        }

        fn process_notice(
            &self,
            input: ProcessNoticeInput,
        ) -> crate::ImResult<ProcessNoticeOutput> {
            self.processed_notices
                .borrow_mut()
                .push(format!("{}:{}", input.group_did, input.from_epoch));
            let mut status = self.status.borrow_mut();
            status.epoch = Some("2".to_owned());
            status.local_epoch = Some("2".to_owned());
            Ok(ProcessNoticeOutput {
                crypto_group_id_b64u: "crypto".to_owned(),
                status: "active".to_owned(),
                self_removed: false,
                from_epoch: input.from_epoch,
                epoch: "2".to_owned(),
                epoch_authenticator: None,
                ratchet_tree_b64u: None,
                subject_did: "did:example:bob".to_owned(),
                subject_status: "active".to_owned(),
            })
        }

        fn generate_key_package(
            &self,
            _input: GenerateKeyPackageInput,
        ) -> crate::ImResult<GroupKeyPackageOutput> {
            unreachable!("repair should not generate key packages")
        }

        fn create_group_prepare(
            &self,
            _input: CreateGroupInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("repair should not create groups")
        }

        fn add_member_prepare(
            &self,
            _input: AddMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("repair should not add members")
        }

        fn remove_member_prepare(
            &self,
            _input: RemoveMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("repair should not remove members")
        }

        fn leave_prepare(
            &self,
            _input: LeaveGroupInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("repair should not leave groups")
        }

        fn update_member_prepare(
            &self,
            _input: UpdateMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("repair should not update members")
        }

        fn recover_member_prepare(
            &self,
            _input: RecoverMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("repair should not recover members")
        }

        fn abort_commit(&self, _input: AbortCommitInput) -> crate::ImResult<AbortCommitOutput> {
            unreachable!("repair should not abort commits")
        }

        fn process_welcome(
            &self,
            _input: ProcessWelcomeInput,
        ) -> crate::ImResult<ProcessWelcomeOutput> {
            unreachable!("commit repair test should not process welcomes")
        }

        fn encrypt(&self, _input: EncryptInput) -> crate::ImResult<EncryptOutput> {
            unreachable!("repair should not encrypt")
        }

        fn decrypt(&self, _input: DecryptInput) -> crate::ImResult<DecryptOutput> {
            unreachable!("repair should not decrypt")
        }
    }

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = unique_temp_root();
            let identities = root.join("identities");
            fs::create_dir_all(&identities).unwrap();
            fs::write(identities.join("default"), "alice\n").unwrap();
            fs::write(
                identities.join("registry.json"),
                r#"{
                  "default_identity": "alice",
                  "identities": [{
                    "id": "alice-id",
                    "did": "did:example:alice",
                    "local_alias": "alice",
                    "ready_for_auth": true,
                    "ready_for_messaging": true,
                    "missing": []
                  }]
                }"#,
            )
            .unwrap();
            fs::create_dir_all(identities.join("alice")).unwrap();
            Self { root }
        }

        fn client(&self) -> crate::core::ImClient {
            crate::core::ImCore::new(
                crate::ImCoreConfig {
                    service_base_url: crate::ServiceEndpoint::parse("https://example.test")
                        .unwrap(),
                    did_domain: "awiki.test".to_owned(),
                    user_service_endpoint: None,
                    message_service_endpoint: None,
                    mail_service_endpoint: None,
                    anp_service_endpoint: None,
                    anp_service_did: None,
                    ca_bundle: None,
                    transport_policy: crate::MessageTransportPolicy::HttpOnly,
                },
                crate::ImCorePaths {
                    identities: crate::paths::IdentityRegistryPaths {
                        identity_root_dir: self.root.join("identities"),
                        registry_path: self.root.join("identities").join("registry.json"),
                        default_identity_path: Some(self.root.join("identities").join("default")),
                    },
                    local_state: crate::paths::LocalStatePaths {
                        sqlite_path: self.root.join("local").join("im.sqlite"),
                    },
                    runtime: crate::paths::RuntimePaths {
                        cache_dir: self.root.join("cache"),
                        temp_dir: self.root.join("tmp"),
                    },
                },
            )
            .unwrap()
            .client(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_owned(),
            ))
            .unwrap()
        }

        fn credentials(&self) -> GroupTextCredentials {
            let bundle = anp::authentication::create_did_wba_document(
                "awiki.test",
                anp::authentication::DidDocumentOptions {
                    path_segments: vec!["user".to_owned()],
                    domain: Some("awiki.test".to_owned()),
                    challenge: Some("group-e2ee-repair-test".to_owned()),
                    ..anp::authentication::DidDocumentOptions::default()
                },
            )
            .unwrap();
            let key1_private_pem = bundle.private_key_pem("key-1").unwrap().to_owned();
            GroupTextCredentials {
                identity_name: "alice".to_owned(),
                did_document: Some(bundle.did_document),
                key1_private_pem,
            }
        }
    }

    fn active_status(epoch: &str, pending_commits: Vec<PendingCommitStatus>) -> StatusOutput {
        StatusOutput {
            status: "active".to_owned(),
            epoch: Some(epoch.to_owned()),
            local_epoch: Some(epoch.to_owned()),
            pending_commits,
            epoch_authenticator: None,
        }
    }

    fn unique_temp_root() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "im-core-group-e2ee-repair-{}-{nanos}",
            std::process::id()
        ))
    }
}
