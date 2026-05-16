use super::service::{auth_session, i64_value, require_active_identity, string_value};
use super::{
    build_group_e2ee_head_rpc_params, build_group_e2ee_notice_rpc_params, Client, CommandResult,
    MessageError, MESSAGE_RPC_ENDPOINT,
};
use crate::config::Resolved;
use crate::identity::types::StoredIdentity;
use crate::identity::Manager;
use crate::transportcfg::Profile;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const ANP_MLS_API_VERSION: &str = "anp-mls/v1";
const DEFAULT_ANP_MLS_BINARY: &str = "anp-mls";
const DEFAULT_ANP_MLS_TIMEOUT: Duration = Duration::from_secs(15);
pub const ANP_MLS_BINARY_ENV: &str = "AWIKI_ANP_MLS_BINARY";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupE2eeStatusRequest {
    pub identity_name: String,
    pub group: String,
    pub limit: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupE2eePendingRequest {
    pub identity_name: String,
    pub group: String,
    pub limit: i64,
}

#[derive(Debug, Clone)]
struct MlsExecProvider {
    binary_path: String,
    data_dir: PathBuf,
}

impl MlsExecProvider {
    fn new(resolved: &Resolved) -> Self {
        Self {
            binary_path: String::new(),
            data_dir: default_mls_data_dir(resolved),
        }
    }

    fn status(
        &self,
        agent_did: &str,
        device_id: &str,
        group_did: &str,
    ) -> Result<Map<String, Value>, MessageError> {
        let request = json!({
            "api_version": ANP_MLS_API_VERSION,
            "request_id": format!("group-e2ee-status-{}", super::wire::generate_operation_id()),
            "agent_did": agent_did,
            "device_id": device_id,
            "params": {
                "agent_did": agent_did,
                "device_id": device_id,
                "group_did": group_did,
            },
        });
        let response = self.call("group", "status", &request, agent_did, device_id)?;
        response
            .get("result")
            .and_then(Value::as_object)
            .cloned()
            .ok_or_else(|| MessageError::Internal("anp-mls response missing result".to_string()))
    }

    fn call(
        &self,
        domain: &str,
        action: &str,
        request: &Value,
        agent_did: &str,
        device_id: &str,
    ) -> Result<Value, MessageError> {
        let binary = self.resolve_binary_path()?;
        let data_dir = self.effective_data_dir(agent_did, device_id);
        if !data_dir.as_os_str().is_empty() {
            fs::create_dir_all(&data_dir).map_err(|err| {
                MessageError::Internal(format!(
                    "prepare anp-mls data dir {}: {err}",
                    data_dir.to_string_lossy()
                ))
            })?;
        }
        let body =
            serde_json::to_vec(request).map_err(|err| MessageError::Json(err.to_string()))?;
        let mut command = Command::new(binary);
        command
            .args([domain, action, "--json-in", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if !data_dir.as_os_str().is_empty() {
            command.arg("--data-dir").arg(&data_dir);
        }
        let mut child = command
            .spawn()
            .map_err(|err| MessageError::Internal(format!("anp-mls exec failed: {err}")))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(&body).map_err(|err| {
                MessageError::Internal(format!("anp-mls exec stdin failed: {err}"))
            })?;
        }
        let output = wait_with_timeout(child, DEFAULT_ANP_MLS_TIMEOUT)?;
        if !output.status.success() && output.stdout.is_empty() {
            return Err(MessageError::Internal(format!(
                "anp-mls exec failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let response: Value = serde_json::from_slice(&output.stdout).map_err(|err| {
            MessageError::Internal(format!(
                "decode anp-mls response: {err}: stderr={}",
                String::from_utf8_lossy(&output.stderr)
            ))
        })?;
        if !response
            .get("ok")
            .and_then(Value::as_bool)
            .unwrap_or_default()
        {
            if let Some(error) = response.get("error").and_then(Value::as_object) {
                let code = string_value(error.get("code"));
                let message = string_value(error.get("message"));
                return Err(MessageError::Internal(format!(
                    "anp-mls error {code}: {message}"
                )));
            }
            return Err(MessageError::Internal(
                "anp-mls returned ok=false".to_string(),
            ));
        }
        Ok(response)
    }

    fn resolve_binary_path(&self) -> Result<String, MessageError> {
        let mut candidates = Vec::new();
        if let Ok(raw) = std::env::var(ANP_MLS_BINARY_ENV) {
            if !raw.trim().is_empty() {
                candidates.push(raw.trim().to_string());
            }
        }
        if !self.binary_path.trim().is_empty() {
            candidates.push(self.binary_path.trim().to_string());
        }
        candidates.push(DEFAULT_ANP_MLS_BINARY.to_string());

        let mut seen = HashSet::new();
        for candidate in candidates {
            if !seen.insert(candidate.clone()) {
                continue;
            }
            let path = Path::new(&candidate);
            if path.is_absolute() || candidate.contains(std::path::MAIN_SEPARATOR) {
                if is_executable_file(path) {
                    return Ok(candidate);
                }
                continue;
            }
            if let Some(found) = find_on_path(&candidate) {
                return Ok(found);
            }
        }
        Err(MessageError::Internal(format!(
            "unable to locate anp-mls binary (checked {ANP_MLS_BINARY_ENV}, injected path, then PATH). Set {ANP_MLS_BINARY_ENV} to an absolute anp-mls path, build/install anp-mls, or run `awiki-cli doctor` for diagnostics"
        )))
    }

    fn effective_data_dir(&self, agent_did: &str, device_id: &str) -> PathBuf {
        if self.data_dir.as_os_str().is_empty() {
            return PathBuf::new();
        }
        let device_id = default_string(device_id.trim(), "default");
        self.data_dir
            .join("agents")
            .join(mls_agent_key(agent_did))
            .join(safe_mls_path_component(&device_id))
    }

    fn candidate_device_ids(&self, agent_did: &str) -> Vec<String> {
        let agent_did = agent_did.trim();
        if agent_did.is_empty() {
            return vec!["default".to_string()];
        }
        let mut candidates = vec!["default".to_string()];
        let agent_dir = self.data_dir.join("agents").join(mls_agent_key(agent_did));
        let Ok(entries) = fs::read_dir(agent_dir) else {
            return candidates;
        };
        let mut seen = HashSet::new();
        seen.insert("default".to_string());
        let mut device_ids = Vec::new();
        for entry in entries.flatten() {
            if !entry.file_type().map(|ty| ty.is_dir()).unwrap_or_default() {
                continue;
            }
            let device_id = entry.file_name().to_string_lossy().trim().to_string();
            if device_id.is_empty() || !seen.insert(device_id.clone()) {
                continue;
            }
            device_ids.push(device_id);
        }
        device_ids.sort();
        for device_id in device_ids {
            candidates.push(device_id);
        }
        candidates
    }
}

pub fn default_mls_data_dir(resolved: &Resolved) -> PathBuf {
    if resolved.paths.workspace_home_dir.trim().is_empty() {
        return PathBuf::from(".awiki-cli").join("mls");
    }
    Path::new(&resolved.paths.workspace_home_dir).join("mls")
}

pub fn inspect_group_e2ee_status(
    resolved: &Resolved,
    manager: &Manager,
    request: GroupE2eeStatusRequest,
) -> Result<CommandResult, MessageError> {
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let limit = if request.limit <= 0 {
        50
    } else {
        request.limit
    };
    let provider = MlsExecProvider::new(resolved);
    let mut warnings = Vec::new();
    let (local_status, local_device_id, local_error) =
        match group_e2ee_status_for_recovery(&provider, &record.did, &request.group, "") {
            Ok((status, device_id)) => (Some(status), device_id, None),
            Err(err) => {
                warnings.push(format!("Group E2EE local MLS status unavailable: {err}"));
                (None, String::new(), Some(err.to_string()))
            }
        };

    let mut service_head: Option<Map<String, Value>> = None;
    let mut pending: Option<Map<String, Value>> = None;
    match group_e2ee_transport(resolved, manager, &record) {
        Ok(mut transport) => {
            match transport.get_group_e2ee_head(&request.group) {
                Ok(head) => service_head = Some(head),
                Err(err) => warnings.push(format!("Group E2EE service head unavailable: {err}")),
            }
            match transport.pull_group_e2ee_notices(&request.group, limit, false) {
                Ok(result) => pending = Some(result),
                Err(err) => warnings.push(format!(
                    "Group E2EE pending notice status unavailable: {err}"
                )),
            }
        }
        Err(err) => warnings.push(format!("Group E2EE service status unavailable: {err}")),
    }

    let pending_notices = pending
        .as_ref()
        .map(|value| values_from_array(value.get("notices")))
        .unwrap_or_default();
    let pending_notice_count = pending
        .as_ref()
        .and_then(|value| i64_value(value.get("pending_count")))
        .unwrap_or(pending_notices.len() as i64);
    let local_status_value = local_status
        .as_ref()
        .map(|value| Value::Object(value.clone()))
        .unwrap_or(Value::Null);
    let service_head_value = service_head
        .as_ref()
        .map(|value| Value::Object(value.clone()))
        .unwrap_or(Value::Null);
    let diagnosis = group_e2ee_recovery_diagnosis(
        local_status.as_ref(),
        service_head.as_ref(),
        pending_notice_count,
        local_error.as_deref(),
    );
    let recovery_artifact = group_e2ee_recovery_artifact(
        &record,
        &request.group,
        &local_device_id,
        local_status.as_ref(),
        service_head.as_ref(),
        &diagnosis,
    );

    Ok(CommandResult {
        data: json!({
            "group": request.group,
            "available": local_error.is_none(),
            "mls": local_status_value,
            "local": local_status_value,
            "local_device_id": local_device_id,
            "service_head": service_head_value,
            "pending_notices": pending_notices,
            "pending_notice_count": pending_notice_count,
            "diagnosis": Value::Object(diagnosis),
            "recovery_artifact": recovery_artifact,
        }),
        summary: "Group E2EE recovery status inspected".to_string(),
        warnings: super::compact_warnings(warnings),
    })
}

pub fn pull_group_e2ee_notices(
    resolved: &Resolved,
    manager: &Manager,
    request: GroupE2eePendingRequest,
) -> Result<CommandResult, MessageError> {
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let limit = if request.limit <= 0 {
        50
    } else {
        request.limit
    };
    let mut transport = group_e2ee_transport(resolved, manager, &record)?;
    let result = transport.pull_group_e2ee_notices(&request.group, limit, false)?;
    Ok(CommandResult {
        data: json!({
            "notices": values_from_array(result.get("notices")),
            "pending_count": result.get("pending_count").cloned().unwrap_or(Value::Null),
            "group": request.group,
        }),
        summary: "Pulled group E2EE pending notices".to_string(),
        warnings: Vec::new(),
    })
}

struct GroupE2eeTransport<'a> {
    client: Client,
    auth: crate::authsdk::Session,
    record: &'a StoredIdentity,
}

impl<'a> GroupE2eeTransport<'a> {
    fn get_group_e2ee_head(&mut self, group_did: &str) -> Result<Map<String, Value>, MessageError> {
        let params = build_group_e2ee_head_rpc_params(self.record, group_did)?;
        self.client.authenticated_rpc_call_profile(
            Profile::RpcDefault,
            MESSAGE_RPC_ENDPOINT,
            "group.e2ee.head",
            params,
            &mut self.auth,
        )
    }

    fn pull_group_e2ee_notices(
        &mut self,
        group_did: &str,
        limit: i64,
        mark_delivered: bool,
    ) -> Result<Map<String, Value>, MessageError> {
        let params = build_group_e2ee_notice_rpc_params(
            self.record,
            group_did,
            limit,
            mark_delivered,
            Vec::new(),
        )?;
        self.client.authenticated_rpc_call_profile(
            Profile::RpcDefault,
            MESSAGE_RPC_ENDPOINT,
            "group.e2ee.notice",
            params,
            &mut self.auth,
        )
    }
}

fn group_e2ee_transport<'a>(
    resolved: &Resolved,
    manager: &Manager,
    record: &'a StoredIdentity,
) -> Result<GroupE2eeTransport<'a>, MessageError> {
    Ok(GroupE2eeTransport {
        client: Client::new(resolved)?,
        auth: auth_session(resolved, manager, record)?,
        record,
    })
}

fn group_e2ee_status_for_recovery(
    provider: &MlsExecProvider,
    agent_did: &str,
    group_did: &str,
    preferred_device_id: &str,
) -> Result<(Map<String, Value>, String), MessageError> {
    let device_ids = group_e2ee_recovery_device_ids(provider, agent_did, preferred_device_id);
    let mut best: Option<Map<String, Value>> = None;
    let mut best_device_id = "default".to_string();
    let mut best_rank = -1;
    let mut best_epoch = -1;
    let mut last_error: Option<MessageError> = None;
    for device_id in device_ids {
        let status = match provider.status(agent_did, &device_id, group_did) {
            Ok(status) => status,
            Err(err) => {
                last_error = Some(err);
                continue;
            }
        };
        let rank = group_e2ee_status_rank(&status);
        let epoch = group_e2ee_local_epoch_from_status(&status).unwrap_or(-1);
        if best.is_none() || rank > best_rank || (rank == best_rank && epoch > best_epoch) {
            best = Some(status);
            best_device_id = device_id;
            best_rank = rank;
            best_epoch = epoch;
        }
    }
    if let Some(mut best) = best {
        best.insert(
            "device_id".to_string(),
            Value::String(best_device_id.clone()),
        );
        return Ok((best, best_device_id));
    }
    if let Some(err) = last_error {
        return Err(err);
    }
    let mut empty = Map::new();
    empty.insert("status".to_string(), Value::String("empty".to_string()));
    empty.insert(
        "device_id".to_string(),
        Value::String(best_device_id.clone()),
    );
    Ok((empty, best_device_id))
}

fn group_e2ee_recovery_device_ids(
    provider: &MlsExecProvider,
    agent_did: &str,
    preferred_device_id: &str,
) -> Vec<String> {
    let mut ordered = Vec::new();
    let mut seen = HashSet::new();
    push_device_id(&mut ordered, &mut seen, preferred_device_id);
    for device_id in provider.candidate_device_ids(agent_did) {
        push_device_id(&mut ordered, &mut seen, &device_id);
    }
    if ordered.is_empty() {
        push_device_id(&mut ordered, &mut seen, "default");
    }
    ordered
}

fn push_device_id(ordered: &mut Vec<String>, seen: &mut HashSet<String>, device_id: &str) {
    let device_id = default_string(device_id.trim(), "default");
    if seen.insert(device_id.clone()) {
        ordered.push(device_id);
    }
}

fn group_e2ee_status_rank(status: &Map<String, Value>) -> i32 {
    match string_value(status.get("status"))
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "active" => 3,
        "left" | "removed" | "inactive" => 2,
        "empty" | "" => {
            if group_e2ee_local_epoch_from_status(status).is_some() {
                1
            } else {
                0
            }
        }
        _ => {
            if group_e2ee_local_epoch_from_status(status).is_some() {
                1
            } else {
                0
            }
        }
    }
}

fn group_e2ee_recovery_diagnosis(
    local_status: Option<&Map<String, Value>>,
    service_head: Option<&Map<String, Value>>,
    pending_notice_count: i64,
    local_error: Option<&str>,
) -> Map<String, Value> {
    let mut diagnosis = Map::new();
    diagnosis.insert("state".to_string(), Value::String("unknown".to_string()));
    diagnosis.insert(
        "next_action".to_string(),
        Value::String("inspect".to_string()),
    );
    diagnosis.insert("fail_closed".to_string(), Value::Bool(true));
    diagnosis.insert(
        "pending_notice_count".to_string(),
        json!(pending_notice_count),
    );
    if let Some(local_error) = local_error.filter(|value| !value.trim().is_empty()) {
        diagnosis.insert(
            "local_error".to_string(),
            Value::String(local_error.to_string()),
        );
    }
    if let Some(local_text) = local_status
        .map(|status| string_value(status.get("status")))
        .filter(|value| !value.trim().is_empty())
    {
        diagnosis.insert("local_status".to_string(), Value::String(local_text));
    }
    if let Some(service_head) = service_head {
        if let Some(value) = service_head.get("actor_membership_status") {
            diagnosis.insert("actor_membership_status".to_string(), value.clone());
        }
        if let Some(value) = service_head.get("actor_recovery_eligible") {
            diagnosis.insert("actor_recovery_eligible".to_string(), value.clone());
        }
        if let Some(value) = service_head.get("epoch") {
            diagnosis.insert("service_epoch".to_string(), value.clone());
        }
    }
    if let Some(local_epoch) = local_status.and_then(group_e2ee_local_epoch_from_status) {
        diagnosis.insert("local_epoch".to_string(), json!(local_epoch.to_string()));
    }
    let pending_commit_count = local_status
        .map(|status| values_from_array(status.get("pending_commits")).len() as i64)
        .unwrap_or_default();
    diagnosis.insert(
        "pending_commit_count".to_string(),
        json!(pending_commit_count),
    );

    let actor_status = service_head
        .map(|head| string_value(head.get("actor_membership_status")))
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if matches!(
        actor_status.as_str(),
        "removed" | "left" | "non_member" | "inactive"
    ) {
        diagnosis.insert("state".to_string(), Value::String("inactive".to_string()));
        diagnosis.insert(
            "next_action".to_string(),
            Value::String("fresh_normal_keypackage_then_group_add_e2ee".to_string()),
        );
        diagnosis.insert(
            "rejoin_command".to_string(),
            Value::String("group add --e2ee".to_string()),
        );
        diagnosis.insert("recover_member_allowed".to_string(), Value::Bool(false));
        diagnosis.insert("fail_closed".to_string(), Value::Bool(true));
        return diagnosis;
    }
    if pending_commit_count > 0 {
        diagnosis.insert(
            "state".to_string(),
            Value::String("pending_commit".to_string()),
        );
        diagnosis.insert(
            "next_action".to_string(),
            Value::String("run_group_e2ee_repair".to_string()),
        );
        diagnosis.insert("fail_closed".to_string(), Value::Bool(false));
        return diagnosis;
    }
    if pending_notice_count > 0 {
        diagnosis.insert(
            "state".to_string(),
            Value::String("pending_notices".to_string()),
        );
        diagnosis.insert(
            "next_action".to_string(),
            Value::String("run_group_e2ee_repair".to_string()),
        );
        diagnosis.insert("fail_closed".to_string(), Value::Bool(false));
        return diagnosis;
    }

    let local_epoch = local_status.and_then(group_e2ee_local_epoch_from_status);
    let service_epoch = service_head.and_then(|head| i64_value(head.get("epoch")));
    let local_state = local_status
        .map(|status| string_value(status.get("status")))
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if local_error.is_some()
        || local_state.is_empty()
        || local_state == "empty"
        || local_epoch.is_none()
    {
        diagnosis.insert(
            "state".to_string(),
            Value::String("missing_state".to_string()),
        );
        diagnosis.insert(
            "next_action".to_string(),
            Value::String("needs_snapshot_or_readd".to_string()),
        );
        diagnosis.insert(
            "active_recovery_command".to_string(),
            Value::String("group e2ee recover-member".to_string()),
        );
        diagnosis.insert(
            "removed_left_rejoin_command".to_string(),
            Value::String("group add --e2ee".to_string()),
        );
        diagnosis.insert("fail_closed".to_string(), Value::Bool(true));
        return diagnosis;
    }
    if let (Some(local_epoch), Some(service_epoch)) = (local_epoch, service_epoch) {
        match local_epoch.cmp(&service_epoch) {
            std::cmp::Ordering::Equal => {
                diagnosis.insert("state".to_string(), Value::String("in_sync".to_string()));
                diagnosis.insert("next_action".to_string(), Value::String("none".to_string()));
                diagnosis.insert("fail_closed".to_string(), Value::Bool(false));
            }
            std::cmp::Ordering::Less => {
                diagnosis.insert("state".to_string(), Value::String("epoch_lag".to_string()));
                diagnosis.insert("epoch_gap".to_string(), json!(service_epoch - local_epoch));
                diagnosis.insert(
                    "next_action".to_string(),
                    Value::String("needs_snapshot_or_readd".to_string()),
                );
                diagnosis.insert("fail_closed".to_string(), Value::Bool(true));
            }
            std::cmp::Ordering::Greater => {
                diagnosis.insert(
                    "state".to_string(),
                    Value::String("local_ahead".to_string()),
                );
                diagnosis.insert("epoch_gap".to_string(), json!(local_epoch - service_epoch));
                diagnosis.insert(
                    "next_action".to_string(),
                    Value::String("stop_and_inspect".to_string()),
                );
                diagnosis.insert("fail_closed".to_string(), Value::Bool(true));
            }
        }
        return diagnosis;
    }
    diagnosis.insert("state".to_string(), Value::String("local_only".to_string()));
    diagnosis.insert(
        "next_action".to_string(),
        Value::String("inspect_service_head".to_string()),
    );
    diagnosis.insert("fail_closed".to_string(), Value::Bool(true));
    diagnosis
}

fn group_e2ee_recovery_artifact(
    record: &StoredIdentity,
    group_did: &str,
    device_id: &str,
    local_status: Option<&Map<String, Value>>,
    service_head: Option<&Map<String, Value>>,
    diagnosis: &Map<String, Value>,
) -> Value {
    if string_value(diagnosis.get("next_action")) != "needs_snapshot_or_readd" {
        return Value::Null;
    }
    let device_id = default_string(device_id.trim(), "default");
    let mut artifact = Map::new();
    artifact.insert(
        "recovery_type".to_string(),
        Value::String("same-device-owner-assisted".to_string()),
    );
    artifact.insert(
        "group_did".to_string(),
        Value::String(group_did.to_string()),
    );
    artifact.insert(
        "member_did".to_string(),
        Value::String(record.did.to_string()),
    );
    artifact.insert("device_id".to_string(), Value::String(device_id.clone()));
    artifact.insert(
        "diagnosis_state".to_string(),
        Value::String(string_value(diagnosis.get("state"))),
    );
    artifact.insert(
        "fail_closed".to_string(),
        diagnosis
            .get("fail_closed")
            .cloned()
            .unwrap_or(Value::Bool(true)),
    );
    artifact.insert("p4_membership_mutate".to_string(), Value::Bool(false));
    artifact.insert(
        "publish_command".to_string(),
        Value::String(format!(
            "group e2ee publish-key-package --recovery --group {group_did} --device {device_id}"
        )),
    );
    artifact.insert(
        "owner_command".to_string(),
        Value::String(format!(
            "group e2ee recover-member --group {group_did} --member {} --device {device_id}",
            record.did
        )),
    );
    if let Some(local_epoch) = local_status.and_then(group_e2ee_local_epoch_from_status) {
        artifact.insert("local_epoch".to_string(), json!(local_epoch.to_string()));
    }
    if let Some(service_head) = service_head {
        if let Some(value) = service_head.get("epoch") {
            artifact.insert("service_epoch".to_string(), value.clone());
        }
        if let Some(value) = service_head.get("actor_membership_status") {
            artifact.insert("member_status".to_string(), value.clone());
        }
    }
    Value::Object(artifact)
}

fn group_e2ee_local_epoch_from_status(status: &Map<String, Value>) -> Option<i64> {
    for key in ["epoch", "local_epoch"] {
        if let Some(epoch) = i64_value(status.get(key)) {
            return Some(epoch);
        }
    }
    for binding in values_from_array(status.get("bindings")) {
        if let Some(epoch) = i64_value(binding.get("epoch")) {
            return Some(epoch);
        }
    }
    None
}

fn values_from_array(value: Option<&Value>) -> Vec<Value> {
    value.and_then(Value::as_array).cloned().unwrap_or_default()
}

fn default_string(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn mls_agent_key(agent_did: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(agent_did.as_bytes());
    let digest = hasher.finalize();
    URL_SAFE_NO_PAD.encode(digest).chars().take(24).collect()
}

fn safe_mls_path_component(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return "default".to_string();
    }
    let sanitized: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    default_string(&sanitized, "default")
}

fn find_on_path(binary: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(binary);
        if is_executable_file(&candidate) {
            return Some(candidate.to_string_lossy().into_owned());
        }
        #[cfg(windows)]
        {
            for extension in ["exe", "bat", "cmd"] {
                let candidate = dir.join(format!("{binary}.{extension}"));
                if is_executable_file(&candidate) {
                    return Some(candidate.to_string_lossy().into_owned());
                }
            }
        }
    }
    None
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if metadata.is_dir() {
        return false;
    }
    #[cfg(windows)]
    {
        true
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
}

struct MlsCommandOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn wait_with_timeout(
    mut child: Child,
    timeout: Duration,
) -> Result<MlsCommandOutput, MessageError> {
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_reader = thread::spawn(move || read_pipe(stdout));
    let stderr_reader = thread::spawn(move || read_pipe(stderr));
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Ok(MlsCommandOutput {
                    status,
                    stdout: join_reader(stdout_reader),
                    stderr: join_reader(stderr_reader),
                });
            }
            Ok(None) if started.elapsed() < timeout => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = join_reader(stdout_reader);
                let stderr = join_reader(stderr_reader);
                return Err(MessageError::Internal(format!(
                    "anp-mls exec failed: timed out after {}s: {}",
                    timeout.as_secs(),
                    String::from_utf8_lossy(&stderr)
                )));
            }
            Err(err) => {
                let _ = join_reader(stdout_reader);
                let _ = join_reader(stderr_reader);
                return Err(MessageError::Internal(format!(
                    "anp-mls exec failed: {err}"
                )));
            }
        }
    }
}

fn read_pipe(pipe: Option<impl Read>) -> Vec<u8> {
    let mut bytes = Vec::new();
    if let Some(mut pipe) = pipe {
        let _ = pipe.read_to_end(&mut bytes);
    }
    bytes
}

fn join_reader(handle: thread::JoinHandle<Vec<u8>>) -> Vec<u8> {
    handle.join().unwrap_or_default()
}
