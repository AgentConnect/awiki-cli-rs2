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
                "backend.{name}.unconfirmed"
            ))),
        }
    }
    pub fn from_message_discovery(raw: &Value) -> crate::ImResult<Self> {
        check_size(raw)?;
        let Some(local) = raw.get("awiki_local") else {
            return Ok(Self::empty());
        };
        if local.get("capabilities_version").and_then(Value::as_u64) != Some(1) {
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
            capabilities,
        })
    }
    pub fn from_server_info(raw: &Value) -> crate::ImResult<Self> {
        check_size(raw)?;
        if raw.get("capabilities_version").is_none() {
            return Ok(Self::empty());
        }
        if raw.get("schema_version").and_then(Value::as_u64) != Some(1)
            || raw.get("capabilities_version").and_then(Value::as_u64) != Some(1)
        {
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

/// Owned by one immutable Core or exact identity runtime, never process-global.
pub(crate) struct CapabilityCache {
    generation: AtomicU64,
    state: tokio::sync::Mutex<CacheState>,
}
static NEXT_RUNTIME_GENERATION: AtomicU64 = AtomicU64::new(1);
impl Default for CapabilityCache {
    fn default() -> Self {
        Self {
            state: Default::default(),
            generation: AtomicU64::new(NEXT_RUNTIME_GENERATION.fetch_add(1, Ordering::Relaxed)),
        }
    }
}
#[derive(Default)]
struct CacheState {
    snapshot: Option<CapabilitySnapshot>,
    success_at: Option<Instant>,
    retry_at: Option<Instant>,
    failures: u32,
    completed_at: Option<Instant>,
}
impl CapabilityCache {
    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
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
        state.success_at = Some(Instant::now());
        state.retry_at = None;
        state.failures = 0;
    }
    pub(crate) async fn invalidate_authorization(&self) {
        let mut state = self.state.lock().await;
        *state = CacheState::default();
        self.generation.store(
            NEXT_RUNTIME_GENERATION.fetch_add(1, Ordering::Relaxed),
            Ordering::Release,
        );
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
        // Keep the lease across I/O: concurrent callers share a successful result and backoff.
        let requested_at = Instant::now();
        let mut state = self.state.lock().await;
        let now = Instant::now();
        if state.completed_at.is_some_and(|at| at >= requested_at) {
            return Ok(if state.failures == 0 {
                state
                    .snapshot
                    .clone()
                    .unwrap_or_else(CapabilitySnapshot::empty)
            } else {
                stale(&state)
            });
        }
        if !force
            && state.failures == 0
            && state
                .success_at
                .is_some_and(|at| now.duration_since(at) < Duration::from_secs(FRESH_SECONDS))
        {
            return Ok(state
                .snapshot
                .clone()
                .unwrap_or_else(CapabilitySnapshot::empty));
        }
        if state.retry_at.is_some_and(|at| at > now) {
            return Ok(stale(&state));
        }
        let result = tokio::time::timeout(Duration::from_secs(10), fetch()).await;
        match result {
            Ok(Ok(snapshot)) if snapshot.status == DiscoveryStatus::Confirmed => {
                state.completed_at = Some(Instant::now());
                state.snapshot = Some(snapshot.clone());
                state.success_at = Some(Instant::now());
                state.retry_at = None;
                state.failures = 0;
                Ok(snapshot)
            }
            Ok(Ok(snapshot)) if snapshot.status == DiscoveryStatus::Incompatible => {
                state.completed_at = Some(Instant::now());
                state.snapshot = Some(snapshot.clone());
                state.success_at = None;
                state.retry_at = Some(Instant::now() + Duration::from_secs(30));
                Ok(snapshot)
            }
            Ok(Err(
                error @ (crate::ImError::PermissionDenied
                | crate::ImError::AuthRequired
                | crate::ImError::SessionExpired),
            )) => Err(error),
            Ok(Err(
                error @ crate::ImError::Service {
                    status_code: Some(401 | 403),
                    ..
                },
            )) => Err(error),
            _ => {
                state.completed_at = Some(Instant::now());
                state.failures = state.failures.saturating_add(1);
                state.retry_at = Some(
                    Instant::now() + Duration::from_secs((5u64 << state.failures.min(6)).min(300)),
                );
                Ok(stale(&state))
            }
        }
    }
}
fn stale(state: &CacheState) -> CapabilitySnapshot {
    let mut snapshot = state
        .snapshot
        .clone()
        .unwrap_or_else(CapabilitySnapshot::empty);
    if snapshot.status == DiscoveryStatus::Confirmed {
        snapshot.status = DiscoveryStatus::Stale;
    }
    snapshot.expired = true;
    snapshot
}

impl crate::ImCore {
    /// Blocking discovery for synchronous consumers; never nests a Tokio runtime.
    pub fn backend_capabilities(&self, force: bool) -> crate::ImResult<CapabilitySnapshot> {
        let core = self.clone();
        blocking_discovery(async move { core.backend_capabilities_async(force).await })
    }
    /// Public discovery before login. This Core's endpoint and storage scope are immutable.
    pub async fn backend_capabilities_async(
        &self,
        force: bool,
    ) -> crate::ImResult<CapabilitySnapshot> {
        let mut snapshot = self
            .inner()
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
            .await?;
        snapshot.context = Some(capability_context(
            self.inner(),
            false,
            self.inner()
                .backend_capabilities
                .generation
                .load(Ordering::Acquire),
        ));
        Ok(snapshot)
    }
}
impl crate::ImClient {
    pub fn backend_capabilities(&self, force: bool) -> crate::ImResult<CapabilitySnapshot> {
        let client = self.clone();
        blocking_discovery(async move { client.backend_capabilities_async(force).await })
    }
    /// Authenticated local declaration, isolated from public federation broadcasts.
    pub async fn backend_capabilities_async(
        &self,
        force: bool,
    ) -> crate::ImResult<CapabilitySnapshot> {
        let generation = self.backend_capabilities.generation.load(Ordering::Acquire);
        let mut snapshot = self
            .discover_backend_with_transport(
                &mut crate::internal::transport::CoreHttpTransport::new(self),
                force,
            )
            .await?;
        if generation != self.backend_capabilities.generation.load(Ordering::Acquire) {
            return Err(crate::ImError::IdentityBindingConflict {
                detail: "capability authorization context changed".into(),
            });
        }
        snapshot.context = Some(capability_context(self.core_inner(), true, generation));
        Ok(snapshot)
    }

    pub(crate) async fn discover_backend_with_transport<T: AsyncAuthenticatedRpcTransport>(
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
    let mut digest = Sha256::new();
    digest.update(serde_json::to_vec(core.sdk_config()).unwrap_or_default());
    digest.update(
        core.sdk_paths()
            .local_state
            .sqlite_path
            .to_string_lossy()
            .as_bytes(),
    );
    digest.update(CONTRACT_VERSION.to_be_bytes());
    CapabilityContext {
        tenant_scope: format!("{:x}", digest.finalize()),
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
