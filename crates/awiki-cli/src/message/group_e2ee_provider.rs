use super::service::string_value;
use super::MessageError;
use crate::config::Resolved;
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

pub(crate) const ANP_MLS_API_VERSION: &str = "anp-mls/v1";
const DEFAULT_ANP_MLS_BINARY: &str = "anp-mls";
const DEFAULT_ANP_MLS_TIMEOUT: Duration = Duration::from_secs(15);
pub const ANP_MLS_BINARY_ENV: &str = "AWIKI_ANP_MLS_BINARY";

#[derive(Debug, Clone)]
pub(crate) struct MlsExecProvider {
    binary_path: String,
    data_dir: PathBuf,
}

impl MlsExecProvider {
    pub(crate) fn new(resolved: &Resolved) -> Self {
        Self {
            binary_path: String::new(),
            data_dir: default_mls_data_dir(resolved),
        }
    }

    pub(crate) fn status(
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
        result_object(response)
    }

    pub(crate) fn generate_key_package(
        &self,
        request: &Value,
        agent_did: &str,
        device_id: &str,
    ) -> Result<Map<String, Value>, MessageError> {
        let response = self.call("key-package", "generate", request, agent_did, device_id)?;
        result_object(response)
    }

    pub(crate) fn create_group(
        &self,
        request: &Value,
        agent_did: &str,
        device_id: &str,
    ) -> Result<Map<String, Value>, MessageError> {
        let response = self.call("group", "create", request, agent_did, device_id)?;
        result_object(response)
    }

    pub(crate) fn add_member(
        &self,
        request: &Value,
        agent_did: &str,
        device_id: &str,
    ) -> Result<Map<String, Value>, MessageError> {
        let response = self.call("group", "add-member", request, agent_did, device_id)?;
        result_object(response)
    }

    pub(crate) fn process_welcome(
        &self,
        request: &Value,
        agent_did: &str,
        device_id: &str,
    ) -> Result<Map<String, Value>, MessageError> {
        let response = self.call("welcome", "process", request, agent_did, device_id)?;
        result_object(response)
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

    pub(crate) fn candidate_device_ids(&self, agent_did: &str) -> Vec<String> {
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

pub(crate) fn default_string(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn result_object(response: Value) -> Result<Map<String, Value>, MessageError> {
    response
        .get("result")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| MessageError::Internal("anp-mls response missing result".to_string()))
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
