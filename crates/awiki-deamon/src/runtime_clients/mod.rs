//! Host installation facts, separate from protocol/session/model readiness.
mod process;
use crate::DaemonConfig;
use serde::Serialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Condvar, Mutex, OnceLock},
    time::{Duration, Instant},
};

pub const KINDS: [&str; 7] = [
    "hermes",
    "codex",
    "claude-code",
    "opencode",
    "gemini",
    "kimi",
    "deepseek-harness",
];
const TTL: Duration = Duration::from_secs(30);
const ITEM_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Serialize)]
pub struct ClientInstallation {
    pub kind: String,
    pub status: &'static str,
    pub version: Option<String>,
    pub reason_code: Option<&'static str>,
}
impl ClientInstallation {
    fn result(kind: &str, result: Result<String, &'static str>) -> Self {
        match result {
            Ok(text) => Self {
                kind: kind.into(),
                status: "ready",
                version: version(&text),
                reason_code: None,
            },
            Err(code) => Self {
                kind: kind.into(),
                status: match code {
                    "not_found" => "missing",
                    "timeout" | "custom_launcher" => "unknown",
                    _ => "unavailable",
                },
                version: None,
                reason_code: Some(code),
            },
        }
    }
}

fn version(text: &str) -> Option<String> {
    static PATTERN: OnceLock<regex::Regex> = OnceLock::new();
    PATTERN
        .get_or_init(|| {
            regex::Regex::new(r"\b\d{1,4}\.\d{1,4}\.\d{1,4}(?:-[A-Za-z0-9.]{1,24})?\b").unwrap()
        })
        .find(text)
        .map(|v| v.as_str().to_owned())
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
    let item = if let Some(binary) = binary_override {
        ClientInstallation::result(
            kind,
            probe_version(
                &crate::cli_runtime_env::resolve_cli_binary(binary),
                deadline,
            ),
        )
    } else {
        inspect_one_until(config, kind, deadline)
    };
    anyhow::ensure!(
        item.status == "ready",
        "runtime_client_{}:{}",
        item.reason_code.unwrap_or("unavailable"),
        kind
    );
    Ok(())
}

fn inspect_one_until(config: &DaemonConfig, kind: &str, deadline: Instant) -> ClientInstallation {
    let deadline = deadline.min(Instant::now() + ITEM_TIMEOUT);
    let result = if Instant::now() >= deadline {
        Err("timeout")
    } else if kind == "hermes" {
        inspect_hermes(config, deadline)
    } else {
        let name = match kind {
            "claude-code" => "claude",
            "deepseek-harness" => "dsh",
            name => name,
        };
        let binary = crate::cli_runtime_env::resolve_cli_binary(name);
        probe_version(&binary, deadline)
    };
    ClientInstallation::result(kind, result)
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

fn inspect_hermes(config: &DaemonConfig, deadline: Instant) -> Result<String, &'static str> {
    let candidates = crate::plugins::hermes::gateway::installation_probe_candidates(config)
        .map_err(|_| "custom_launcher")?;
    let mut failure = "not_found";
    for parts in candidates {
        if Instant::now() >= deadline {
            return Err("timeout");
        }
        // Only the adapter's known Python module form has a safe local probe.
        if parts.len() != 3 || parts[1] != "-m" || parts[2] != "tui_gateway.entry" {
            return Err("custom_launcher");
        }
        let python = crate::cli_runtime_env::resolve_cli_binary(&parts[0]);
        if !python.is_file() {
            continue;
        }
        let mut command = Command::new(python);
        command.args(["-B", "-c", r#"import sys, importlib.machinery as m
sys.path = [p for p in sys.path if p not in ('', '.')]
p = m.PathFinder.find_spec('tui_gateway', sys.path)
e = m.PathFinder.find_spec('tui_gateway.entry', p.submodule_search_locations) if p and p.submodule_search_locations else None
sys.exit(0 if e else 2)"#]);
        if let Some(path) = crate::cli_runtime_env::cli_child_path() {
            command.env("PATH", path);
        }
        match process::run(&mut command, deadline) {
            Ok(_) => return Ok(String::new()),
            Err("version_failed") => failure = "gateway_module_missing",
            Err(code) => failure = code,
        }
    }
    Err(failure)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
