//! Host installation facts, separate from protocol/session/model readiness.
use crate::runtime::probe as process;
use crate::DaemonConfig;
use serde::Serialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Condvar, Mutex, OnceLock},
    time::{Duration, Instant},
};

pub const KINDS: [&str; 7] = crate::acp::SUPPORTED_DRIVERS;
const TTL: Duration = Duration::from_secs(30);
const ITEM_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Serialize)]
pub struct ClientInstallation {
    pub kind: String,
    pub status: &'static str,
    pub version: Option<String>,
    pub reason_code: Option<&'static str>,
    pub execution_protocol: &'static str,
    pub adapter_version: Option<String>,
}
impl ClientInstallation {
    fn result(kind: &str, result: Result<String, &'static str>) -> Self {
        match result {
            Ok(text) => Self {
                kind: kind.into(),
                status: "ready",
                version: version(&text),
                reason_code: None,
                execution_protocol: "acp",
                adapter_version: None,
            },
            Err(code) => Self {
                kind: kind.into(),
                status: match code {
                    "not_found" => "missing",
                    "timeout" => "unknown",
                    _ => "unavailable",
                },
                version: None,
                reason_code: Some(code),
                execution_protocol: "acp",
                adapter_version: None,
            },
        }
    }
}

fn version(text: &str) -> Option<String> {
    static PATTERN: OnceLock<regex::Regex> = OnceLock::new();
    PATTERN
        .get_or_init(|| {
            regex::Regex::new(r"\bv?(\d{1,4}\.\d{1,4}\.\d{1,4}(?:-[A-Za-z0-9.]{1,24})?)\b").unwrap()
        })
        .captures(text)
        .map(|v| v[1].to_owned())
}

#[derive(Clone, Debug, Serialize)]
pub struct InstallationSnapshot {
    pub schema_version: u8,
    pub checked_at_ms: i64,
    pub cache_age_ms: u64,
    pub clients: Vec<ClientInstallation>,
}

#[derive(Default)]
struct CacheState {
    running: bool,
    value: Option<(Instant, InstallationSnapshot)>,
}
#[derive(Default)]
pub struct InstallationCache {
    state: Mutex<CacheState>,
    done: Condvar,
}
impl InstallationCache {
    fn inspect(
        &self,
        refresh: bool,
        probe: impl FnOnce() -> InstallationSnapshot,
    ) -> InstallationSnapshot {
        let mut state = self.state.lock().unwrap();
        let mut joined = false;
        while state.running {
            joined = true;
            state = self.done.wait(state).unwrap();
        }
        if let Some((when, snapshot)) = &state.value {
            if joined || !refresh && when.elapsed() < TTL {
                let mut snapshot = snapshot.clone();
                snapshot.cache_age_ms = when.elapsed().as_millis() as u64;
                return snapshot;
            }
        }
        state.running = true;
        drop(state);
        // Never leave waiters stranded if a client adapter panics.
        let value =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(probe)).unwrap_or_else(|_| {
                snapshot(
                    KINDS
                        .iter()
                        .map(|k| ClientInstallation::result(k, Err("launch_failed")))
                        .collect(),
                )
            });
        let mut state = self.state.lock().unwrap();
        state.value = Some((Instant::now(), value.clone()));
        state.running = false;
        self.done.notify_all();
        value
    }
}

pub fn inspect(config: &DaemonConfig, refresh: bool) -> InstallationSnapshot {
    static CACHES: OnceLock<Mutex<HashMap<PathBuf, Arc<InstallationCache>>>> = OnceLock::new();
    let cache = CACHES
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .entry(config.state_root.clone())
        .or_default()
        .clone();
    cache.inspect(refresh, || {
        let deadline = Instant::now() + Duration::from_secs(20);
        let next = std::sync::atomic::AtomicUsize::new(0);
        let results = Mutex::new(Vec::new());
        std::thread::scope(|scope| {
            for _ in 0..3 {
                scope.spawn(|| loop {
                    let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(kind) = KINDS.get(index) else {
                        break;
                    };
                    let result = inspect_one_until(config, kind, deadline);
                    results.lock().unwrap().push((index, result));
                });
            }
        });
        let mut results = results.into_inner().unwrap();
        results.sort_by_key(|r| r.0);
        snapshot(results.into_iter().map(|r| r.1).collect())
    })
}

fn snapshot(clients: Vec<ClientInstallation>) -> InstallationSnapshot {
    InstallationSnapshot {
        schema_version: 1,
        checked_at_ms: crate::security::runtime_token::current_time_millis().unwrap_or(0),
        cache_age_ms: 0,
        clients,
    }
}

pub fn require_installed(
    config: &DaemonConfig,
    kind: &str,
    binary_override: Option<&str>,
) -> anyhow::Result<()> {
    let deadline = Instant::now() + ITEM_TIMEOUT;
    let _ = config;
    let item = inspect_client(
        kind,
        binary_override.map(crate::cli_runtime_env::resolve_cli_binary),
        deadline,
    );
    anyhow::ensure!(
        item.status == "ready",
        "runtime_client_{}:{}",
        item.reason_code.unwrap_or("unavailable"),
        kind
    );
    Ok(())
}

fn inspect_one_until(_config: &DaemonConfig, kind: &str, deadline: Instant) -> ClientInstallation {
    inspect_client(kind, None, deadline.min(Instant::now() + ITEM_TIMEOUT))
}

fn inspect_client(kind: &str, binary: Option<PathBuf>, deadline: Instant) -> ClientInstallation {
    let brand = match crate::acp::Brand::parse(kind) {
        Ok(brand) => brand,
        Err(_) => return ClientInstallation::result(kind, Err("unsupported_client")),
    };
    let binary =
        binary.unwrap_or_else(|| crate::cli_runtime_env::resolve_cli_binary(brand.command()));
    let result = if Instant::now() >= deadline {
        Err("timeout")
    } else if brand == crate::acp::Brand::Hermes {
        inspect_hermes(&binary, deadline)
    } else {
        probe_version(&binary, deadline)
    };
    let mut item = ClientInstallation::result(kind, result);
    if item.status == "ready"
        && matches!(
            brand,
            crate::acp::Brand::Codex | crate::acp::Brand::ClaudeCode
        )
    {
        match inspect_adapter(brand, deadline) {
            Ok(version) => item.adapter_version = Some(version),
            Err(code) => {
                item.status = "unavailable";
                item.reason_code = Some(code);
            }
        }
    }
    item
}

fn inspect_adapter(brand: crate::acp::Brand, deadline: Instant) -> Result<String, &'static str> {
    let adapter = crate::acp::components::Adapter::discover(brand).map_err(|error| match error
        .to_string()
        .as_str()
    {
        "acp_adapter_platform_unsupported" => "adapter_platform_unsupported",
        "acp_adapter_missing" => "adapter_missing",
        _ => "adapter_invalid",
    })?;
    let output =
        probe_version(&adapter.node, deadline).map_err(|_| "adapter_runtime_unavailable")?;
    if version(&output).as_deref() != Some(adapter.node_version.as_str()) {
        return Err("adapter_invalid");
    }
    Ok(adapter.version)
}

fn probe_version(binary: &Path, deadline: Instant) -> Result<String, &'static str> {
    if !binary.is_file() {
        return Err("not_found");
    }
    let mut command = Command::new(binary);
    command.arg("--version");
    if let Some(path) = crate::cli_runtime_env::cli_child_path() {
        command.env("PATH", path);
    }
    process::run(&mut command, deadline)
}

fn inspect_hermes(binary: &Path, deadline: Instant) -> Result<String, &'static str> {
    if !binary.is_file() {
        return Err("not_found");
    }
    let mut command = Command::new(binary);
    command.args(["acp", "--version"]);
    if let Some(path) = crate::cli_runtime_env::cli_child_path() {
        command.env("PATH", path);
    }
    let version = process::run(&mut command, deadline)?;
    if Instant::now() >= deadline {
        return Err("timeout");
    }
    let mut check = Command::new(binary);
    check.args(["acp", "--check"]);
    if let Some(path) = crate::cli_runtime_env::cli_child_path() {
        check.env("PATH", path);
    }
    process::run(&mut check, deadline).map_err(|code| {
        if code == "version_failed" {
            "acp_dependencies_missing"
        } else {
            code
        }
    })?;
    Ok(version)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

#[cfg(test)]
mod hermes_tests;
