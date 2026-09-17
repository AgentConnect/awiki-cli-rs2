//! Home-selected synchronization mode, bound to the current local identity.
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::internal::transport::AsyncAuthenticatedRpcTransport;
pub(crate) use crate::internal::wire::sync_v2::community::SyncServiceMode;

pub(crate) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS sync_service_modes (
    owner_identity_id TEXT PRIMARY KEY REFERENCES identity_account_bindings(owner_identity_id) ON DELETE CASCADE,
    mode TEXT NOT NULL CHECK(mode IN ('commercial','community')),
    binding_json TEXT NOT NULL,
    service_did TEXT NOT NULL,
    capabilities_json TEXT NOT NULL
);";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ModeBinding {
    home: String,
    account: String,
    did: String,
    device: String,
    key: String,
    generation: String,
    installation: String,
}

fn error(message: &str) -> crate::ImError {
    crate::ImError::IdentityBindingConflict {
        detail: message.to_owned(),
    }
}

pub(crate) fn guard_request(client: &crate::core::ImClient, params: &Value) -> crate::ImResult<()> {
    if cached_mode(client)? != Some(SyncServiceMode::Community) {
        return Ok(());
    }
    let profile = params
        .pointer("/meta/profile")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let security = params
        .pointer("/meta/security_profile")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if profile.contains(".e2ee.") || matches!(security, "direct-e2ee" | "group-e2ee") {
        return Err(crate::ImError::unsupported("community-e2ee"));
    }
    Ok(())
}

fn binding(client: &crate::core::ImClient, installation: String) -> crate::ImResult<ModeBinding> {
    let account = client.sync_account_context()?;
    let config = client.core_inner().sdk_config();
    Ok(ModeBinding {
        home: config
            .message_service_endpoint
            .as_ref()
            .unwrap_or(&config.service_base_url)
            .as_str()
            .trim_end_matches('/')
            .to_owned(),
        account: account.account_id,
        did: client.did().as_str().to_owned(),
        device: account.protocol_device_id,
        key: client.runtime().key_provider.request_signing_key_id()?,
        generation: account.device_auth_generation,
        installation,
    })
}

fn same_identity(left: &ModeBinding, right: &ModeBinding) -> bool {
    left.home == right.home
        && left.account == right.account
        && left.did == right.did
        && left.device == right.device
        && left.key == right.key
        && left.installation == right.installation
}

/// A cached Community record restricts unsupported operations. It is not a
/// substitute for fresh discovery at session/sync initialization or reconnect.
pub(crate) fn cached_mode(
    client: &crate::core::ImClient,
) -> crate::ImResult<Option<SyncServiceMode>> {
    if client.runtime().owner.sync_account.is_none() {
        return Ok(None);
    }
    let path = &client.core_inner().sdk_paths().local_state.sqlite_path;
    if !path.exists() {
        return Ok(None);
    }
    let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let exists: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='sync_service_modes')",
            [],
            |row| row.get(0),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if !exists {
        return Ok(None);
    }
    let row: Option<(String, String)> = db
        .query_row(
            "SELECT mode,binding_json FROM sync_service_modes WHERE owner_identity_id=?1",
            [client.current_identity().id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let Some((mode, saved)) = row else {
        return Ok(None);
    };
    if mode == "commercial" {
        return Ok(Some(SyncServiceMode::Commercial));
    }
    let installation: String = db
        .query_row(
            "SELECT client_instance_id FROM sync_installation_state WHERE owner_identity_id=?1",
            [client.current_identity().id.as_str()],
            |row| row.get(0),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let saved: ModeBinding =
        serde_json::from_str(&saved).map_err(|_| error("stored Community binding is invalid"))?;
    if mode != "community" || !same_identity(&saved, &binding(client, installation)?) {
        return Err(error(
            "Community mode no longer belongs to this Home/account/device/installation",
        ));
    }
    Ok(Some(SyncServiceMode::Community))
}

pub(crate) async fn discover<T: AsyncAuthenticatedRpcTransport>(
    client: &crate::core::ImClient,
    transport: &mut T,
) -> crate::ImResult<SyncServiceMode> {
    let params = crate::internal::wire::sync_v2::build_capability_discovery_params(
        &crate::internal::wire::common::WireIdentity {
            did: client.did().as_str().to_owned(),
        },
    )?;
    let raw = transport
        .authenticated_rpc("/im/rpc", "anp.get_capabilities", params)
        .await?;
    confirm(client, &raw).await
}

/// Old imported root-only identities can use the existing ordinary read RPCs.
/// This never creates an account/device binding or a reliable-sync checkpoint.
fn require_unbound_legacy_owner(db: &Connection, owner: &str) -> crate::ImResult<()> {
    // A missing runtime binding must not downgrade persisted vNext state.
    // Historical databases may not have these tables; do not migrate them here.
    for table in [
        "identity_account_bindings",
        "sync_service_modes",
        "lane_sync_state",
        "sync_lane_inbox",
        "sync_recovery_state",
    ] {
        let exists: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |row| row.get(0),
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        if exists {
            let bound: bool = db
                .query_row(
                    &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE owner_identity_id=?1)"),
                    [owner],
                    |row| row.get(0),
                )
                .map_err(crate::internal::local_state::local_state_unavailable)?;
            if bound {
                return Err(error(
                    "Legacy Community reads conflict with existing vNext owner state",
                ));
            }
        }
    }
    Ok(())
}

pub(crate) async fn legacy_reads(client: &crate::core::ImClient) -> crate::ImResult<bool> {
    if client.runtime().owner.sync_account.is_some()
        || client.current_identity().local_alias.is_none()
        || client
            .runtime()
            .key_provider
            .did_document()?
            .get("deviceManifest")
            .is_some()
    {
        return Ok(false);
    }
    let path = &client.core_inner().sdk_paths().local_state.sqlite_path;
    if path.exists() {
        let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        require_unbound_legacy_owner(&db, client.current_identity().id.as_str())?;
    }
    let config = client.core_inner().sdk_config();
    let user = config
        .user_service_endpoint
        .as_ref()
        .unwrap_or(&config.service_base_url);
    let message = config
        .message_service_endpoint
        .as_ref()
        .unwrap_or(&config.service_base_url);
    let user =
        reqwest::Url::parse(user.as_str()).map_err(|_| error("invalid Community User endpoint"))?;
    let message = reqwest::Url::parse(message.as_str())
        .map_err(|_| error("invalid Community Message endpoint"))?;
    if user.origin() != message.origin() {
        return Err(error(
            "Community User and Message endpoints must have the same origin",
        ));
    }
    let params = crate::internal::wire::sync_v2::build_capability_discovery_params(
        &crate::internal::wire::common::WireIdentity {
            did: client.did().as_str().to_owned(),
        },
    )?;
    let raw = crate::internal::transport::CoreHttpTransport::new(client)
        .authenticated_rpc("/im/rpc", "anp.get_capabilities", params)
        .await?;
    legacy_reads_declaration(&raw, config)
}

fn legacy_reads_declaration(raw: &Value, config: &crate::ImCoreConfig) -> crate::ImResult<bool> {
    if crate::internal::wire::sync_v2::community::discover_sync_service_mode(raw)?
        != SyncServiceMode::Community
    {
        return Ok(false);
    }
    let expected = config
        .anp_service_did
        .as_ref()
        .map(|did| did.as_str().to_owned())
        .unwrap_or_else(|| format!("did:wba:{}", config.did_domain.replace(':', "%3A")));
    if raw.get("service_did").and_then(Value::as_str) != Some(expected.as_str()) {
        return Err(error(
            "Community declaration does not match the configured Home service DID",
        ));
    }
    Ok(true)
}

/// Resolve an unbound session or revalidate a Community session before online
/// messaging. An established commercial session keeps its existing lifecycle.
pub(crate) async fn ensure_session_mode(
    client: &crate::core::ImClient,
) -> crate::ImResult<Option<SyncServiceMode>> {
    if client.runtime().owner.sync_account.is_none() {
        return Ok(None);
    }
    if cached_mode(client)? == Some(SyncServiceMode::Commercial) {
        return Ok(Some(SyncServiceMode::Commercial));
    }
    let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
    let params = crate::internal::wire::sync_v2::build_capability_discovery_params(
        &crate::internal::wire::common::WireIdentity {
            did: client.did().as_str().to_owned(),
        },
    )?;
    let raw = transport
        .authenticated_rpc("/im/rpc", "anp.get_capabilities", params)
        .await?;
    let mode = confirm(client, &raw).await?;
    if mode == SyncServiceMode::Community
        && raw
            .get("supported_profiles")
            .and_then(Value::as_array)
            .is_some_and(|profiles| {
                profiles
                    .iter()
                    .any(|value| value.as_str() == Some("anp.group.base.v2"))
            })
    {
        client
            .core_handle()
            .identities()
            .ensure_community_service_discovery(client)
            .await?;
    }
    Ok(Some(mode))
}

pub(crate) fn ensure_session_mode_sync(
    client: &crate::core::ImClient,
) -> crate::ImResult<Option<SyncServiceMode>> {
    if client.runtime().owner.sync_account.is_none() {
        return Ok(None);
    }
    if cached_mode(client)? == Some(SyncServiceMode::Commercial) {
        return Ok(Some(SyncServiceMode::Commercial));
    }
    let client = client.clone();
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| error("Community session runtime is unavailable"))?
            .block_on(ensure_session_mode(&client))
    })
    .join()
    .map_err(|_| error("Community session worker failed"))?
}

pub(crate) async fn confirm(
    client: &crate::core::ImClient,
    raw: &Value,
) -> crate::ImResult<SyncServiceMode> {
    let mode = crate::internal::wire::sync_v2::community::discover_sync_service_mode(raw)?;
    let config = client.core_inner().sdk_config();
    let expected_service = config
        .anp_service_did
        .as_ref()
        .map(|did| did.as_str().to_owned())
        .unwrap_or_else(|| format!("did:wba:{}", config.did_domain.replace(':', "%3A")));
    if mode == SyncServiceMode::Community
        && raw.get("service_did").and_then(Value::as_str) != Some(expected_service.as_str())
    {
        return Err(error(
            "Community declaration does not match the configured Home service DID",
        ));
    }
    let active = client.active_sync_account_binding().await?;
    let db = client.core_inner().local_state_db().await?;
    let installation = db
        .load_or_create_sync_client_instance_id(&active.owner_identity_id)
        .await?;
    let context = binding(client, installation)?;
    let owner = active.owner_identity_id;
    let service = raw
        .get("service_did")
        .and_then(Value::as_str)
        .unwrap_or(&expected_service)
        .to_owned();
    let capabilities = raw.to_string();
    db.run_local(move |connection| {
        save_mode(connection, &owner, mode, &context, &service, &capabilities)
    })
    .await?;
    Ok(mode)
}

fn save_mode(
    db: &Connection,
    owner: &str,
    mode: SyncServiceMode,
    context: &ModeBinding,
    service: &str,
    capabilities: &str,
) -> crate::ImResult<()> {
    let tx = rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate)
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let selected = if mode == SyncServiceMode::Community {
        "community"
    } else {
        "commercial"
    };
    let previous: Option<(String,String,String)> = tx.query_row("SELECT mode,binding_json,service_did FROM sync_service_modes WHERE owner_identity_id=?1", [owner], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)))
        .optional().map_err(crate::internal::local_state::local_state_unavailable)?;
    if let Some((previous_mode, previous_binding, previous_service)) = previous {
        if previous_mode != selected {
            return Err(error(
                "Home sync mode changed; explicit account reconciliation is required",
            ));
        }
        if mode == SyncServiceMode::Community {
            let previous: ModeBinding = serde_json::from_str(&previous_binding)
                .map_err(|_| error("stored Community binding is invalid"))?;
            if !same_identity(&previous, context) || previous_service != service {
                return Err(error(
                    "Community declaration conflicts with the saved identity binding",
                ));
            }
            let old_generation = previous.generation.parse::<u64>().ok();
            let new_generation = context.generation.parse::<u64>().ok();
            if !matches!((old_generation, new_generation), (Some(old), Some(new)) if old > 0 && new >= old)
            {
                return Err(error("Community auth generation cannot move backwards"));
            }
        }
    }
    if mode == SyncServiceMode::Community {
        let has_lanes: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM lane_sync_state WHERE owner_identity_id=?1)
                OR EXISTS(SELECT 1 FROM sync_lane_inbox WHERE owner_identity_id=?1 AND lane IN ('p5_device','p6_group'))
                OR EXISTS(SELECT 1 FROM sync_recovery_state WHERE owner_identity_id=?1)",
            [owner], |row| row.get(0),
        ).map_err(crate::internal::local_state::local_state_unavailable)?;
        if has_lanes {
            return Err(error(
                "Community mode conflicts with existing lane or recovery state",
            ));
        }
    }
    let encoded =
        serde_json::to_string(context).map_err(|_| error("sync mode binding cannot be encoded"))?;
    tx.execute("INSERT INTO sync_service_modes(owner_identity_id,mode,binding_json,service_did,capabilities_json) VALUES (?1,?2,?3,?4,?5)
        ON CONFLICT(owner_identity_id) DO UPDATE SET mode=excluded.mode,binding_json=excluded.binding_json,service_did=excluded.service_did,capabilities_json=excluded.capabilities_json",
        rusqlite::params![owner,selected,encoded,service,capabilities]).map_err(crate::internal::local_state::local_state_unavailable)?;
    tx.commit()
        .map_err(crate::internal::local_state::local_state_unavailable)
}

#[cfg(test)]
mod tests;
