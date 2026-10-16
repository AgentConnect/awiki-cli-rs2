//! Versioned, redacted home-server capability discovery. A declaration is not authorization.
use crate::internal::transport::{AsyncAuthenticatedRpcTransport, AsyncRestTransport};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{
    collections::BTreeMap,
    future::Future,
    time::{Duration, Instant},
};

pub(crate) mod persistence;
mod worker;
pub(crate) use worker::DiscoveryWorker;

const PUBLIC_KEYS: &[&str] = &[
    "identity.multi_device",
    "identity.device_revoke",
    "identity.capability_upgrade",
    "identity.root_transfer",
    "identity.account_recovery",
    "agents.managed_agents",
    "agents.daemon",
    "agents.skill_onboarding",
    "agents.skill_group_membership",
    "account.state_sync",
    "services.mail",
    "services.model_proxy",
    "services.guest_gateway",
    "content.pages",
    "content.sites",
    "content.hosting",
    "client_updates.policy",
    "registration.phone",
    "registration.email",
    "registration.handle_only",
];

pub const CONTRACT_VERSION: u32 = 1;
pub const FRESH_SECONDS: u64 = 300;
pub const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const MESSAGE_KEYS: &[&str] = &[
    "direct",
    "groups",
    "attachments",
    "history",
    "read_state",
    "sync",
    "snapshot",
    "p5_device",
    "p6_group",
    "direct_e2ee",
    "group_e2ee",
    "encrypted_attachments",
    "group_open_join",
    "group_admin_add",
    "group_invite",
    "group_approval",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    Supported,
    Unsupported,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryStatus {
    Confirmed,
    Stale,
    Unavailable,
    Incompatible,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Capability {
    pub support: Support,
    pub protocol_version: Option<u32>,
    pub reason_code: Option<String>,
    pub limits: BTreeMap<String, Value>,
}
impl Capability {
    fn unknown() -> Self {
        Self {
            support: Support::Unknown,
            protocol_version: None,
            reason_code: None,
            limits: BTreeMap::new(),
        }
    }
    fn parse(value: Option<&Value>) -> crate::ImResult<Self> {
        let Some(value) = value else {
            return Ok(Self::unknown());
        };
        let enabled = value
            .get("enabled")
            .and_then(Value::as_bool)
            .ok_or_else(invalid_contract)?;
        let version = value
            .get("protocol_version")
            .and_then(Value::as_u64)
            .ok_or_else(invalid_contract)?;
        if version != 1 {
            return Ok(Self::unknown());
        }
        // Project only documented non-secret numeric limits and fixed delivery identifiers.
        let mut limits = BTreeMap::new();
        if let Some(raw) = value.get("limits") {
            let raw = raw.as_object().ok_or_else(invalid_contract)?;
            for key in [
                "max_object_bytes",
                "max_members",
                "max_joined_groups",
                "max_devices",
                "max_client_instances",
                "max_pages",
                "max_manifest_attachments",
                "schema",
            ] {
                if let Some(value) = raw.get(key) {
                    let numeric = value.as_u64().is_some()
                        || value.as_str().is_some_and(|v| {
                            !v.is_empty() && v.len() <= 20 && v.bytes().all(|c| c.is_ascii_digit())
                        });
                    if !numeric {
                        return Err(invalid_contract());
                    }
                    limits.insert(key.to_owned(), value.clone());
                }
            }
            if raw.get("delivery").and_then(Value::as_str) == Some("paged_v1") {
                limits.insert("delivery".into(), Value::String("paged_v1".into()));
            }
        }
        Ok(Self {
            support: if enabled {
                Support::Supported
            } else {
                Support::Unsupported
            },
            protocol_version: Some(1),
            reason_code: if enabled {
                None
            } else {
                Some("not_supported".into())
            },
            limits,
        })
    }
}
/// Opaque scope identifiers; no paths, endpoints, identity material or bearer tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityContext {
    pub tenant_scope: String,
    pub authenticated: bool,
    pub runtime_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilitySnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<CapabilityContext>,
    pub capabilities_version: u32,
    pub status: DiscoveryStatus,
    pub fetched_at: Option<i64>,
    pub expired: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_error: Option<CapabilityRefreshError>,
    pub capabilities: BTreeMap<String, Capability>,
}
impl CapabilitySnapshot {
    fn empty() -> Self {
        Self {
            context: None,
            capabilities_version: CONTRACT_VERSION,
            status: DiscoveryStatus::Unavailable,
            fetched_at: None,
            expired: true,
            refresh_error: None,
            capabilities: BTreeMap::new(),
        }
    }
    pub fn support(&self, name: &str) -> Support {
        if matches!(
            self.status,
            DiscoveryStatus::Incompatible | DiscoveryStatus::Unavailable
        ) {
            return Support::Unknown;
        }
        self.capabilities
            .get(name)
            .map_or(Support::Unknown, |c| c.support)
    }
    pub fn limit(&self, name: &str, key: &str) -> Option<u64> {
        if self.support(name) != Support::Supported {
            return None;
        }
        let value = self.capabilities.get(name)?.limits.get(key)?;
        value.as_u64().or_else(|| value.as_str()?.parse().ok())
    }
    pub fn check_limit(&self, name: &str, key: &str, observed: u64) -> crate::ImResult<()> {
        if self
            .limit(name, key)
            .is_some_and(|maximum| observed > maximum)
        {
            return Err(crate::ImError::invalid_input(
                Some(key.to_owned()),
                format!("backend.{name}.limit_exceeded"),
            ));
        }
        Ok(())
    }
    pub fn require(&self, name: &str) -> crate::ImResult<()> {
        match self.support(name) {
            Support::Supported => Ok(()),
            Support::Unsupported => Err(crate::ImError::unsupported(format!(
                "backend.{name}.not_supported"
            ))),
            Support::Unknown => Err(crate::ImError::unsupported(format!(
                "backend.{name}.{}",
                if self.refresh_error == Some(CapabilityRefreshError::UpgradeRequired) {
                    "server_upgrade_required"
                } else {
                    "unconfirmed"
                }
            ))),
        }
    }
    pub fn from_message_discovery(raw: &Value) -> crate::ImResult<Self> {
        check_size(raw)?;
        let Some(local) = raw.get("awiki_local") else {
            return Ok(Self::empty());
        };
        let Some(version) = local.get("capabilities_version") else {
            return Ok(Self::empty());
        };
        let version = version.as_u64().ok_or_else(invalid_contract)?;
        if version != 1 {
            return Ok(Self {
                status: DiscoveryStatus::Incompatible,
                ..Self::empty()
            });
        }
        let values = local
            .get("capabilities")
            .and_then(Value::as_object)
            .ok_or_else(invalid_contract)?;
        let mut capabilities = BTreeMap::new();
        for key in MESSAGE_KEYS {
            capabilities.insert((*key).into(), Capability::parse(values.get(*key))?);
        }
        Ok(Self {
            context: None,
            capabilities_version: 1,
            status: DiscoveryStatus::Confirmed,
            fetched_at: Some(chrono::Utc::now().timestamp()),
            expired: false,
            refresh_error: None,
            capabilities,
        })
    }
    pub fn from_server_info(raw: &Value) -> crate::ImResult<Self> {
        check_size(raw)?;
        if raw.get("capabilities_version").is_none() {
            return Ok(Self::empty());
        }
        let schema = raw
            .get("schema_version")
            .and_then(Value::as_u64)
            .ok_or_else(invalid_contract)?;
        let version = raw
            .get("capabilities_version")
            .and_then(Value::as_u64)
            .ok_or_else(invalid_contract)?;
        if schema != 1 || version != 1 {
            return Ok(Self {
                status: DiscoveryStatus::Incompatible,
                ..Self::empty()
            });
        }
        let mut capabilities = BTreeMap::new();
        for (group, names) in [
            (
                "identity",
                &[
                    "multi_device",
                    "device_revoke",
                    "capability_upgrade",
                    "root_transfer",
                ][..],
            ),
            (
                "agents",
                &[
                    "managed_agents",
                    "daemon",
                    "skill_onboarding",
                    "skill_group_membership",
                ][..],
            ),
            ("account", &["state_sync"][..]),
            ("services", &["mail", "model_proxy", "guest_gateway"][..]),
            ("content", &["pages", "sites", "hosting"][..]),
            ("client_updates", &["policy"][..]),
        ] {
            for name in names {
                capabilities.insert(
                    format!("{group}.{name}"),
                    Capability::parse(raw.get(group).and_then(|v| v.get(name)))?,
                );
            }
        }
        if let Some(recovery) = raw.pointer("/identity/handle_recovery") {
            let methods = recovery
                .get("methods")
                .and_then(Value::as_array)
                .ok_or_else(invalid_contract)?;
            let enabled = methods.iter().any(|method| method["enabled"] == true);
            capabilities.insert(
                "identity.account_recovery".into(),
                Capability {
                    support: if enabled {
                        Support::Supported
                    } else {
                        Support::Unsupported
                    },
                    protocol_version: Some(1),
                    reason_code: if enabled {
                        None
                    } else {
                        Some("not_supported".into())
                    },
                    limits: BTreeMap::new(),
                },
            );
        }
        if let Some(registration) = raw.pointer("/identity/handle_registration") {
            let enabled = registration
                .get("enabled")
                .and_then(Value::as_bool)
                .ok_or_else(invalid_contract)?;
            let methods = registration
                .get("methods")
                .and_then(Value::as_array)
                .ok_or_else(invalid_contract)?;
            for name in ["phone", "email", "handle_only"] {
                let matching: Vec<_> = methods.iter().filter(|v| v["id"] == name).collect();
                if matching.len() > 1 {
                    return Err(invalid_contract());
                }
                let supported = enabled
                    && matching.first().is_some_and(|method| {
                        method["enabled"] == true
                            && (name != "handle_only"
                                || (method["verification"]["required"] == false
                                    && method["verification"]["type"] == "none"))
                    });
                capabilities.insert(
                    format!("registration.{name}"),
                    Capability {
                        support: if supported {
                            Support::Supported
                        } else {
                            Support::Unsupported
                        },
                        protocol_version: Some(1),
                        reason_code: if supported {
                            None
                        } else {
                            Some("not_supported".into())
                        },
                        limits: BTreeMap::new(),
                    },
                );
            }
        }
        Ok(Self {
            context: None,
            capabilities_version: 1,
            status: DiscoveryStatus::Confirmed,
            fetched_at: Some(chrono::Utc::now().timestamp()),
            expired: false,
            refresh_error: None,
            capabilities,
        })
    }
}
fn invalid_contract() -> crate::ImError {
    crate::ImError::invalid_input(Some("capabilities".into()), "backend_capabilities_invalid")
}
fn check_size(raw: &Value) -> crate::ImResult<()> {
    if serde_json::to_vec(raw)
        .map_err(|_| invalid_contract())?
        .len()
        > MAX_RESPONSE_BYTES
    {
        return Err(invalid_contract());
    }
    Ok(())
}

/// Safe state notifications contain no transport errors or authentication material.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilityState {
    pub snapshot: CapabilitySnapshot,
    pub refresh_error: Option<CapabilityRefreshError>,
    pub persistence_failed: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityRefreshError {
    Unavailable,
    AuthenticationRequired,
    UpgradeRequired,
}

/// Network serialization and snapshot access have separate locks.
pub(crate) struct CapabilityCache {
    generation: AtomicU64,
    state: tokio::sync::Mutex<CacheState>,
    refresh: tokio::sync::Mutex<()>,
    scheduled: std::sync::atomic::AtomicBool,
    cancellation: std::sync::Mutex<tokio_util::sync::CancellationToken>,
    persistence: Option<persistence::Persistence>,
    persistence_enabled: std::sync::atomic::AtomicBool,
    changes: tokio::sync::watch::Sender<CapabilityState>,
}
static NEXT_RUNTIME_GENERATION: AtomicU64 = AtomicU64::new(1);
impl Default for CapabilityCache {
    fn default() -> Self {
        Self::new(None)
    }
}
#[derive(Default)]
struct CacheState {
    snapshot: Option<CapabilitySnapshot>,
    success_at: Option<Instant>,
    retry_at: Option<Instant>,
    failures: u32,
    completed_at: Option<Instant>,
    revision: u64,
    refresh_error: Option<CapabilityRefreshError>,
    persistence_failed: bool,
}
impl CapabilityCache {
    fn new(persistence: Option<persistence::Persistence>) -> Self {
        let snapshot = persistence.as_ref().and_then(|p| p.read());
        let age = snapshot
            .as_ref()
            .and_then(|s| s.fetched_at)
            .map(|at| chrono::Utc::now().timestamp().saturating_sub(at));
        let success_at = age
            .filter(|age| *age >= 0 && *age < FRESH_SECONDS as i64)
            .and_then(|age| Instant::now().checked_sub(Duration::from_secs(age as u64)));
        let state = CacheState {
            snapshot,
            success_at,
            ..Default::default()
        };
        let (changes, _) = tokio::sync::watch::channel(CapabilityState {
            snapshot: current(&state),
            refresh_error: None,
            persistence_failed: false,
        });
        Self {
            state: tokio::sync::Mutex::new(state),
            refresh: Default::default(),
            scheduled: Default::default(),
            cancellation: Default::default(),
            persistence,
            persistence_enabled: std::sync::atomic::AtomicBool::new(true),
            changes,
            generation: AtomicU64::new(NEXT_RUNTIME_GENERATION.fetch_add(1, Ordering::Relaxed)),
        }
    }
    pub(crate) fn public(config: &crate::ImCoreConfig, paths: &crate::ImCorePaths) -> Self {
        Self::new(Some(persistence::Persistence::public(config, paths)))
    }
    pub(crate) fn identity(
        core: &crate::core::ImCoreInner,
        runtime: &crate::internal::identity_runtime::ClientIdentityRuntime,
    ) -> Self {
        Self::new(Some(persistence::Persistence::identity(core, runtime)))
    }
    fn cancellation_token(&self) -> tokio_util::sync::CancellationToken {
        self.cancellation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
    pub(crate) fn stop(&self) {
        self.cancellation_token().cancel();
        self.generation.store(
            NEXT_RUNTIME_GENERATION.fetch_add(1, Ordering::Relaxed),
            Ordering::Release,
        );
    }
    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
    async fn snapshot(&self) -> CapabilitySnapshot {
        current(&*self.state.lock().await)
    }
    fn schedule<F, Fut>(self: &std::sync::Arc<Self>, worker: &DiscoveryWorker, fetch: F)
    where
        F: Fn() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        if self.scheduled.swap(true, Ordering::AcqRel) {
            return;
        }
        struct Scheduled(std::sync::Arc<CapabilityCache>);
        impl Drop for Scheduled {
            fn drop(&mut self) {
                self.0.scheduled.store(false, Ordering::Release);
            }
        }
        let scheduled = Scheduled(self.clone());
        let generation = self.generation();
        let cancellation = self.cancellation_token();
        let _ = worker.spawn(async move {
            let cache = &scheduled.0;
            loop {
                let delay = {
                    let state = cache.state.lock().await;
                    if generation != cache.generation()
                        || fresh(&state)
                        || state.refresh_error
                            == Some(CapabilityRefreshError::AuthenticationRequired)
                    {
                        break;
                    }
                    state.retry_at.map_or(Duration::ZERO, |at| {
                        at.saturating_duration_since(Instant::now())
                    })
                };
                tokio::select! {
                    _ = cancellation.cancelled() => break,
                    _ = tokio::time::sleep(delay) => {}
                }
                if generation != cache.generation() || fresh(&*cache.state.lock().await) {
                    break;
                }
                fetch().await;
            }
        });
    }
    fn publish(&self, state: &CacheState) {
        self.changes.send_replace(CapabilityState {
            snapshot: current(state),
            refresh_error: state.refresh_error,
            persistence_failed: state.persistence_failed,
        });
    }
    fn persist(&self, state: &mut CacheState) {
        if !self.persistence_enabled.load(Ordering::Acquire) {
            return;
        }
        if let (Some(persistence), Some(snapshot)) = (&self.persistence, &state.snapshot) {
            state.persistence_failed = persistence.write(snapshot).is_err();
        }
    }
    pub(crate) async fn observe_rpc_result(
        &self,
        generation: u64,
        method: &str,
        result: &crate::ImResult<Value>,
    ) {
        let key = match method {
            "direct.e2ee.publish_prekey_bundle" => "direct_e2ee",
            "group.e2ee.publish_key_package" => "group_e2ee",
            _ => return,
        };
        let Err(crate::ImError::Service {
            status_code,
            code,
            message,
            ..
        }) = result
        else {
            return;
        };
        if matches!(status_code, Some(401 | 403))
            || !(code.as_deref() == Some("not_supported")
                || (code.as_deref() == Some("-32010") && message == "not_supported"))
        {
            return;
        }
        let mut state = self.state.lock().await;
        if generation != self.generation() {
            return;
        }
        let mut snapshot = state
            .snapshot
            .take()
            .unwrap_or_else(CapabilitySnapshot::empty);
        snapshot.status = DiscoveryStatus::Confirmed;
        snapshot.fetched_at = Some(chrono::Utc::now().timestamp());
        snapshot.expired = false;
        snapshot.capabilities.insert(
            key.into(),
            Capability {
                support: Support::Unsupported,
                protocol_version: Some(1),
                reason_code: Some("not_supported".into()),
                limits: BTreeMap::new(),
            },
        );
        state.snapshot = Some(snapshot);
        state.revision += 1;
        state.success_at = Some(Instant::now());
        state.retry_at = None;
        state.failures = 0;
        state.refresh_error = None;
        self.persist(&mut state);
        self.publish(&state);
    }
    pub(crate) async fn invalidate_authorization(&self) {
        // A mutable session advanced beyond this immutable client's disk binding.
        // The replacement client will persist under its own verified context.
        self.persistence_enabled.store(false, Ordering::Release);
        let mut state = self.state.lock().await;
        let revision = state.revision + 1;
        *state = CacheState {
            revision,
            ..Default::default()
        };
        self.generation.store(
            NEXT_RUNTIME_GENERATION.fetch_add(1, Ordering::Relaxed),
            Ordering::Release,
        );
        self.publish(&state);
    }
    pub(crate) async fn discover<F, Fut>(
        &self,
        force: bool,
        fetch: F,
    ) -> crate::ImResult<CapabilitySnapshot>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = crate::ImResult<CapabilitySnapshot>>,
    {
        // Ordinary reads never wait on an outstanding request, even when stale.
        if !force && self.state.lock().await.snapshot.is_some() {
            return Ok(self.snapshot().await);
        }
        let requested_at = Instant::now();
        let generation = self.generation();
        let _lease = self.refresh.lock().await;
        let revision = {
            let state = self.state.lock().await;
            if generation != self.generation() {
                return Err(context_changed());
            }
            if state.completed_at.is_some_and(|at| at >= requested_at)
                || (!force && fresh(&state))
                || state.retry_at.is_some_and(|at| at > Instant::now())
            {
                return match state.refresh_error {
                    Some(CapabilityRefreshError::AuthenticationRequired) => {
                        Err(crate::ImError::AuthRequired)
                    }
                    _ => Ok(current(&state)),
                };
            }
            state.revision
        };
        let cancellation = self.cancellation_token();
        let result = tokio::select! {
            _ = cancellation.cancelled() => return Err(context_changed()),
            result = tokio::time::timeout(Duration::from_secs(10), fetch()) => result,
        };
        let mut state = self.state.lock().await;
        if generation != self.generation() {
            return Err(context_changed());
        }
        // An explicit business rejection committed after this request began wins.
        if revision != state.revision {
            return Ok(current(&state));
        }
        state.completed_at = Some(Instant::now());
        match result {
            Ok(Ok(mut snapshot))
                if matches!(
                    snapshot.status,
                    DiscoveryStatus::Confirmed | DiscoveryStatus::Incompatible
                ) =>
            {
                snapshot.fetched_at = Some(chrono::Utc::now().timestamp());
                state.snapshot = Some(snapshot);
                state.success_at = Some(Instant::now());
                state.retry_at = None;
                state.failures = 0;
                state.refresh_error = None;
                state.revision += 1;
                self.persist(&mut state);
            }
            Ok(Err(error)) if authentication_error(&error) => {
                state.refresh_error = Some(CapabilityRefreshError::AuthenticationRequired);
                self.publish(&state);
                return Err(error);
            }
            result => {
                let missing_contract = matches!(result, Ok(Ok(snapshot)) if snapshot.status == DiscoveryStatus::Unavailable);
                state.failures = state.failures.saturating_add(1);
                state.retry_at = Some(
                    Instant::now() + Duration::from_secs((5u64 << state.failures.min(6)).min(300)),
                );
                state.refresh_error = Some(if missing_contract {
                    CapabilityRefreshError::UpgradeRequired
                } else {
                    CapabilityRefreshError::Unavailable
                });
            }
        }
        self.publish(&state);
        Ok(current(&state))
    }
}
fn context_changed() -> crate::ImError {
    crate::ImError::IdentityBindingConflict {
        detail: "capability authorization context changed".into(),
    }
}
fn authentication_error(error: &crate::ImError) -> bool {
    matches!(
        error,
        crate::ImError::PermissionDenied
            | crate::ImError::AuthRequired
            | crate::ImError::SessionExpired
            | crate::ImError::Service {
                status_code: Some(401 | 403),
                ..
            }
    ) || matches!(error, crate::ImError::Service { code: Some(code), .. } if matches!(code.as_str(), "1401" | "1403"))
}
fn fresh(state: &CacheState) -> bool {
    state.failures == 0
        && state
            .success_at
            .is_some_and(|at| at.elapsed() < Duration::from_secs(FRESH_SECONDS))
}
fn current(state: &CacheState) -> CapabilitySnapshot {
    let mut snapshot = state
        .snapshot
        .clone()
        .unwrap_or_else(CapabilitySnapshot::empty);
    snapshot.refresh_error = state.refresh_error;
    if !fresh(state) {
        if snapshot.status == DiscoveryStatus::Confirmed {
            snapshot.status = DiscoveryStatus::Stale;
        }
        snapshot.expired = true;
    }
    snapshot
}

impl crate::ImCore {
    pub fn stop_backend_discovery(&self) {
        self.inner().backend_capabilities.stop();
        let sessions = self
            .inner()
            .identity_capability_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for cache in sessions
            .values()
            .flatten()
            .filter_map(|cache| cache.upgrade())
        {
            cache.stop();
        }
        self.inner().capability_worker.stop();
    }

    pub fn backend_capabilities(&self, force: bool) -> crate::ImResult<CapabilitySnapshot> {
        let core = self.clone();
        blocking_discovery(async move { core.backend_capabilities_async(force).await })
    }
    pub async fn backend_capabilities_snapshot_async(&self) -> CapabilitySnapshot {
        let mut snapshot = self.inner().backend_capabilities.snapshot().await;
        snapshot.context = Some(capability_context(
            self.inner(),
            false,
            self.inner().backend_capabilities.generation(),
        ));
        snapshot
    }
    pub fn subscribe_backend_capabilities(&self) -> tokio::sync::watch::Receiver<CapabilityState> {
        self.inner().backend_capabilities.changes.subscribe()
    }
    pub async fn backend_capabilities_async(
        &self,
        force: bool,
    ) -> crate::ImResult<CapabilitySnapshot> {
        let cache = &self.inner().backend_capabilities;
        if cache.cancellation_token().is_cancelled() {
            return Err(context_changed());
        }
        if force || cache.state.lock().await.snapshot.is_none() {
            self.refresh_backend_capabilities(force).await?;
        }
        let weak = self.weak_handle();
        cache.schedule(&self.inner().capability_worker, move || {
            let core = weak();
            async move {
                if let Some(core) = core {
                    let _ = core.refresh_backend_capabilities(true).await;
                }
            }
        });
        Ok(self.backend_capabilities_snapshot_async().await)
    }
    async fn refresh_backend_capabilities(
        &self,
        force: bool,
    ) -> crate::ImResult<CapabilitySnapshot> {
        self.inner()
            .backend_capabilities
            .discover(force, || async {
                let query = BTreeMap::from([
                    ("capabilities_version".into(), "1".into()),
                    (
                        "registration_domain".into(),
                        self.inner().sdk_config().did_domain.clone(),
                    ),
                ]);
                let raw = crate::internal::transport::CorePlainTransport::new(self)
                    .rest_get("/user-service/v1/server-info", "GET", &query)
                    .await?;
                CapabilitySnapshot::from_server_info(&raw)
            })
            .await
    }
}
impl crate::ImClient {
    pub fn backend_capabilities(&self, force: bool) -> crate::ImResult<CapabilitySnapshot> {
        let client = self.clone();
        blocking_discovery(async move { client.backend_capabilities_async(force).await })
    }
    pub async fn backend_capabilities_snapshot_async(&self) -> CapabilitySnapshot {
        let mut snapshot = self.backend_capabilities.snapshot().await;
        snapshot.context = Some(capability_context(
            self.core_inner(),
            true,
            self.backend_capabilities.generation(),
        ));
        snapshot
    }
    pub fn subscribe_backend_capabilities(&self) -> tokio::sync::watch::Receiver<CapabilityState> {
        self.backend_capabilities.changes.subscribe()
    }
    /// Hosts stop authenticated discovery when signing out; persisted facts remain
    /// available when the same valid local identity is explicitly reopened.
    pub fn stop_backend_capabilities(&self) {
        self.backend_capabilities.stop();
    }
    pub fn resume_backend_capabilities(&self) {
        let mut token = self
            .backend_capabilities
            .cancellation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if token.is_cancelled() {
            *token = tokio_util::sync::CancellationToken::new();
        }
    }
    pub async fn backend_capabilities_async(
        &self,
        force: bool,
    ) -> crate::ImResult<CapabilitySnapshot> {
        self.discover_backend_with_transport(
            &mut crate::internal::transport::CoreHttpTransport::new(self),
            force,
        )
        .await?;
        Ok(self.backend_capabilities_snapshot_async().await)
    }
    pub(crate) async fn discover_backend_with_transport<T: AsyncAuthenticatedRpcTransport>(
        &self,
        transport: &mut T,
        force: bool,
    ) -> crate::ImResult<CapabilitySnapshot> {
        let cache = &self.backend_capabilities;
        if cache.cancellation_token().is_cancelled() {
            return Err(crate::ImError::SessionExpired);
        }
        if force || cache.state.lock().await.snapshot.is_none() {
            self.fetch_backend_with_transport(transport, force).await?;
        }
        let weak = self.weak_handle();
        cache.schedule(&self.core_inner().capability_worker, move || {
            let client = weak();
            async move {
                if let Some(client) = client {
                    let _ = client
                        .fetch_backend_with_transport(
                            &mut crate::internal::transport::CoreHttpTransport::new(&client),
                            true,
                        )
                        .await;
                }
            }
        });
        Ok(cache.snapshot().await)
    }

    async fn fetch_backend_with_transport<T: AsyncAuthenticatedRpcTransport>(
        &self,
        transport: &mut T,
        force: bool,
    ) -> crate::ImResult<CapabilitySnapshot> {
        self.backend_capabilities
            .discover(force, || async {
                let identity = crate::internal::wire::common::WireIdentity {
                    did: self.did().as_str().to_owned(),
                };
                let params =
                    crate::internal::wire::sync_v2::build_capability_discovery_params(&identity)?;
                let raw = transport
                    .authenticated_rpc("/im/rpc", "anp.get_capabilities", params)
                    .await?;
                let snapshot = CapabilitySnapshot::from_message_discovery(&raw)?;
                if snapshot.support("sync") == Support::Supported {
                    crate::internal::wire::sync_v2::require_explicit_sync_negotiation_capability(
                        &raw,
                    )?;
                }
                Ok(snapshot)
            })
            .await
    }
}
fn capability_context(
    core: &crate::core::ImCoreInner,
    authenticated: bool,
    runtime_generation: u64,
) -> CapabilityContext {
    CapabilityContext {
        tenant_scope: persistence::tenant_key(core.sdk_config(), core.sdk_paths()),
        authenticated,
        runtime_generation,
    }
}

fn blocking_discovery<F>(future: F) -> crate::ImResult<CapabilitySnapshot>
where
    F: Future<Output = crate::ImResult<CapabilitySnapshot>> + Send + 'static,
{
    let run = move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| crate::ImError::Internal {
                message: "capability runtime unavailable".into(),
            })?
            .block_on(future)
    };
    if tokio::runtime::Handle::try_current().is_ok() {
        std::thread::spawn(run)
            .join()
            .map_err(|_| crate::ImError::Internal {
                message: "capability worker unavailable".into(),
            })?
    } else {
        run()
    }
}

#[cfg(test)]
mod tests;
