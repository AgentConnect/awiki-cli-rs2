use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::agent::{AgentDefinition, AgentKind};
use crate::service::{manage_service, ServiceAction};
use crate::state::DaemonState;
use crate::DaemonConfig;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeArchiveReport {
    pub archive_id: String,
    pub archive_dir: PathBuf,
    pub moved_paths: Vec<ArchivedPath>,
    pub skipped_paths: Vec<SkippedArchivePath>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonArchiveReport {
    pub archive_id: String,
    pub archive_dir: PathBuf,
    pub finalizer_scheduled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonArchiveFinalizeReport {
    pub archive_id: String,
    pub archived_state_root: PathBuf,
    pub service_uninstalled: bool,
    pub state_root_moved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PendingDaemonArchiveFinalizer {
    archive_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchivedPath {
    pub kind: String,
    pub from: PathBuf,
    pub to: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedArchivePath {
    pub kind: String,
    pub path: PathBuf,
    pub reason: String,
}

pub fn archive_runtime_agent(
    config: &DaemonConfig,
    state: &DaemonState,
    runtime_agent: &AgentDefinition,
) -> Result<RuntimeArchiveReport> {
    if runtime_agent.agent_kind != AgentKind::Runtime {
        bail!("archive_runtime_agent requires a runtime agent");
    }
    let archive_id = archive_id_for(&runtime_agent.agent_did)?;
    let archive_dir = runtime_archive_dir(config, &archive_id);
    std::fs::create_dir_all(&archive_dir)
        .with_context(|| format!("create runtime archive {}", archive_dir.display()))?;

    let mut report = RuntimeArchiveReport {
        archive_id,
        archive_dir,
        moved_paths: Vec::new(),
        skipped_paths: Vec::new(),
    };

    move_state_child(
        config,
        &runtime_agent.local_agent_db_path,
        &report.archive_dir,
        "local_agent_db",
        &mut report.moved_paths,
        &mut report.skipped_paths,
    )?;
    move_state_child(
        config,
        &runtime_agent.message_db_path,
        &report.archive_dir,
        "message_db",
        &mut report.moved_paths,
        &mut report.skipped_paths,
    )?;

    if let Ok(profile) = state.load_hermes_profile(&runtime_agent.agent_did) {
        move_existing_path(
            config,
            &profile.hermes_home,
            &report.archive_dir,
            "hermes_home",
            &mut report.moved_paths,
            &mut report.skipped_paths,
        )?;
    }

    write_manifest(
        &report.archive_dir,
        json!({
            "schema": "awiki.daemon.archive.runtime.v1",
            "archive_id": report.archive_id,
            "agent_did": &runtime_agent.agent_did,
            "agent_kind": runtime_agent.agent_kind.as_str(),
            "handle": &runtime_agent.handle,
            "runtime_profile_id": &runtime_agent.runtime_profile_id,
            "moved_paths": &report.moved_paths,
            "skipped_paths": &report.skipped_paths,
        }),
    )?;
    state.mark_agent_archived(&runtime_agent.agent_did)?;
    state.insert_audit_event_json(
        "runtime.agent.archived",
        Some(&runtime_agent.agent_did),
        runtime_agent.runtime_profile_id.as_deref(),
        None,
        None,
        json!({
            "archive_id": report.archive_id,
            "moved_path_count": report.moved_paths.len(),
            "skipped_path_count": report.skipped_paths.len(),
        }),
    )?;
    Ok(report)
}

pub fn prepare_daemon_archive(
    config: &DaemonConfig,
    state: &DaemonState,
    daemon_agent: &AgentDefinition,
    runtime_agents: &[AgentDefinition],
) -> Result<DaemonArchiveReport> {
    if daemon_agent.agent_kind != AgentKind::Daemon {
        bail!("prepare_daemon_archive requires a daemon agent");
    }
    let archive_id = archive_id_for(&daemon_agent.agent_did)?;
    let archive_dir = daemon_archive_dir(config, &archive_id);
    std::fs::create_dir_all(&archive_dir)
        .with_context(|| format!("create daemon archive {}", archive_dir.display()))?;
    for runtime in runtime_agents {
        state.mark_agent_archived(&runtime.agent_did)?;
    }
    state.mark_agent_archived(&daemon_agent.agent_did)?;
    write_manifest(
        &archive_dir,
        json!({
            "schema": "awiki.daemon.archive.daemon.v1",
            "archive_id": archive_id,
            "daemon_agent_did": &daemon_agent.agent_did,
            "daemon_handle": &daemon_agent.handle,
            "runtime_agent_dids": runtime_agents.iter().map(|agent| agent.agent_did.as_str()).collect::<Vec<_>>(),
            "active_state_root": &config.state_root,
        }),
    )?;
    state.insert_audit_event_json(
        "daemon.agent.archive.prepared",
        Some(&daemon_agent.agent_did),
        None,
        None,
        None,
        json!({
            "archive_id": archive_id,
            "runtime_agent_count": runtime_agents.len(),
        }),
    )?;
    Ok(DaemonArchiveReport {
        archive_id,
        archive_dir,
        finalizer_scheduled: false,
    })
}

pub fn schedule_daemon_archive_finalizer(
    config: DaemonConfig,
    archive_id: String,
    delay: Duration,
) -> Result<()> {
    std::thread::Builder::new()
        .name("awiki-daemon-archive-finalizer".to_string())
        .spawn(move || {
            std::thread::sleep(delay);
            let _ = finalize_daemon_archive_for_foreground_shutdown(&config, &archive_id);
        })
        .context("spawn daemon archive finalizer")?;
    Ok(())
}

pub fn write_pending_daemon_archive_finalizer(
    config: &DaemonConfig,
    archive_id: &str,
) -> Result<()> {
    let pending = PendingDaemonArchiveFinalizer {
        archive_id: archive_id.to_string(),
    };
    let path = pending_daemon_archive_finalizer_path(config);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "create daemon archive finalizer marker parent {}",
                parent.display()
            )
        })?;
    }
    std::fs::write(&path, serde_json::to_vec_pretty(&pending)?)
        .with_context(|| format!("write daemon archive finalizer marker {}", path.display()))
}

pub fn pending_daemon_archive_finalizer(config: &DaemonConfig) -> Result<Option<String>> {
    let path = pending_daemon_archive_finalizer_path(config);
    if !path.exists() {
        return Ok(None);
    }
    let pending: PendingDaemonArchiveFinalizer = serde_json::from_slice(
        &std::fs::read(&path)
            .with_context(|| format!("read daemon archive finalizer marker {}", path.display()))?,
    )
    .with_context(|| format!("parse daemon archive finalizer marker {}", path.display()))?;
    if pending.archive_id.trim().is_empty() {
        bail!("daemon archive finalizer marker archive_id must not be empty");
    }
    Ok(Some(pending.archive_id))
}

pub fn finalize_daemon_archive(
    config: &DaemonConfig,
    archive_id: &str,
) -> Result<DaemonArchiveFinalizeReport> {
    finalize_daemon_archive_with_service_action(config, archive_id, ServiceAction::Uninstall)
}

pub fn finalize_daemon_archive_for_foreground_shutdown(
    config: &DaemonConfig,
    archive_id: &str,
) -> Result<DaemonArchiveFinalizeReport> {
    finalize_daemon_archive_with_service_action(
        config,
        archive_id,
        ServiceAction::RemoveRegistration,
    )
}

fn finalize_daemon_archive_with_service_action(
    config: &DaemonConfig,
    archive_id: &str,
    service_action: ServiceAction,
) -> Result<DaemonArchiveFinalizeReport> {
    let archive_dir = daemon_archive_dir(config, archive_id);
    std::fs::create_dir_all(&archive_dir)
        .with_context(|| format!("create daemon archive {}", archive_dir.display()))?;
    let service_uninstalled = if should_uninstall_product_service(config) {
        let executable = crate::service::default_executable_path()?;
        manage_service(config, &executable, service_action).is_ok()
    } else {
        false
    };
    let archived_state_root = archive_dir.join("state");
    let mut state_root_moved = false;
    if config.state_root.exists() && !archived_state_root.exists() {
        if let Some(parent) = archived_state_root.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create archive parent {}", parent.display()))?;
        }
        std::fs::rename(&config.state_root, &archived_state_root).with_context(|| {
            format!(
                "archive daemon state root {} -> {}",
                config.state_root.display(),
                archived_state_root.display()
            )
        })?;
        state_root_moved = true;
    }
    write_manifest(
        &archive_dir,
        json!({
            "schema": "awiki.daemon.archive.daemon.finalized.v1",
            "archive_id": archive_id,
            "archived_state_root": &archived_state_root,
            "service_uninstalled": service_uninstalled,
            "state_root_moved": state_root_moved,
        }),
    )?;
    Ok(DaemonArchiveFinalizeReport {
        archive_id: archive_id.to_string(),
        archived_state_root,
        service_uninstalled,
        state_root_moved,
    })
}

fn pending_daemon_archive_finalizer_path(config: &DaemonConfig) -> PathBuf {
    config
        .state_root
        .join("run")
        .join("daemon-archive-finalizer.json")
}

fn should_uninstall_product_service(config: &DaemonConfig) -> bool {
    let default_root = DaemonConfig::default_product_state_root().ok();
    state_root_owns_product_service(&config.state_root, default_root.as_deref())
}

fn state_root_owns_product_service(state_root: &Path, default_root: Option<&Path>) -> bool {
    default_root.is_some_and(|default_root| state_root == default_root)
}

fn runtime_archive_dir(config: &DaemonConfig, archive_id: &str) -> PathBuf {
    config
        .state_root
        .join("archive")
        .join("runtime")
        .join(archive_id)
}

fn daemon_archive_dir(config: &DaemonConfig, archive_id: &str) -> PathBuf {
    config
        .state_root
        .parent()
        .unwrap_or(&config.state_root)
        .join("archive")
        .join("daemon")
        .join(archive_id)
}

fn archive_id_for(agent_did: &str) -> Result<String> {
    let now = crate::security::runtime_token::current_time_millis()?;
    Ok(format!("{now}-{}", stable_segment(agent_did)))
}

fn stable_segment(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .chars()
        .take(64)
        .collect::<String>()
}

fn move_state_child(
    config: &DaemonConfig,
    relative_path: &str,
    archive_dir: &Path,
    kind: &str,
    moved: &mut Vec<ArchivedPath>,
    skipped: &mut Vec<SkippedArchivePath>,
) -> Result<()> {
    let trimmed = relative_path.trim();
    if trimmed.is_empty() {
        skipped.push(SkippedArchivePath {
            kind: kind.to_string(),
            path: PathBuf::new(),
            reason: "empty_path".to_string(),
        });
        return Ok(());
    }
    move_existing_path(
        config,
        &config.state_root.join(trimmed),
        archive_dir,
        kind,
        moved,
        skipped,
    )
}

fn move_existing_path(
    config: &DaemonConfig,
    path: &Path,
    archive_dir: &Path,
    kind: &str,
    moved: &mut Vec<ArchivedPath>,
    skipped: &mut Vec<SkippedArchivePath>,
) -> Result<()> {
    if !path.exists() {
        skipped.push(SkippedArchivePath {
            kind: kind.to_string(),
            path: path.to_path_buf(),
            reason: "not_found".to_string(),
        });
        return Ok(());
    }
    if !path_is_under(path, &config.state_root) {
        skipped.push(SkippedArchivePath {
            kind: kind.to_string(),
            path: path.to_path_buf(),
            reason: "outside_state_root".to_string(),
        });
        return Ok(());
    }
    let relative = path.strip_prefix(&config.state_root).unwrap_or(path);
    let destination = archive_dir.join("files").join(relative);
    if destination.exists() {
        skipped.push(SkippedArchivePath {
            kind: kind.to_string(),
            path: path.to_path_buf(),
            reason: "archive_destination_exists".to_string(),
        });
        return Ok(());
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create archive file parent {}", parent.display()))?;
    }
    std::fs::rename(path, &destination).with_context(|| {
        format!(
            "archive {kind} {} -> {}",
            path.display(),
            destination.display()
        )
    })?;
    moved.push(ArchivedPath {
        kind: kind.to_string(),
        from: path.to_path_buf(),
        to: destination,
    });
    Ok(())
}

fn path_is_under(path: &Path, root: &Path) -> bool {
    let normalized_path = canonicalize_existing_prefix(path);
    let normalized_root = canonicalize_existing_prefix(root);
    normalized_path.starts_with(normalized_root)
}

fn canonicalize_existing_prefix(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    let Some(parent) = path.parent() else {
        return path.to_path_buf();
    };
    if parent == path {
        return path.to_path_buf();
    }
    canonicalize_existing_prefix(parent).join(path.file_name().unwrap_or_default())
}

fn write_manifest(dir: &Path, value: serde_json::Value) -> Result<()> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("create archive manifest directory {}", dir.display()))?;
    let path = dir.join("manifest.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&value)?)
        .with_context(|| format!("write archive manifest {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{agent_data_paths, AgentKind};

    #[test]
    fn runtime_archive_moves_owned_state_files_and_marks_archived() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let (local_agent_db_path, message_db_path) = agent_data_paths("did:agent:runtime").unwrap();
        let runtime = AgentDefinition {
            agent_did: "did:agent:runtime".to_string(),
            handle: "runtime".to_string(),
            agent_kind: AgentKind::Runtime,
            controller_user_id: "user-alice".to_string(),
            controller_full_handle: "alice.anpclaw.com".to_string(),
            controller_scope_key: "user:user-alice".to_string(),
            controller_did: "did:human:alice".to_string(),
            runtime_plugin_id: Some("runtime.hermes".to_string()),
            runtime_profile_id: Some("profile_runtime".to_string()),
            workspace_id: None,
            policy_id: "default".to_string(),
            local_agent_db_path,
            message_db_path,
            status: "active".to_string(),
        };
        state.upsert_agent_definition(&runtime).unwrap();
        let agent_db = config.state_root.join(&runtime.local_agent_db_path);
        let message_db = config.state_root.join(&runtime.message_db_path);
        std::fs::create_dir_all(agent_db.parent().unwrap()).unwrap();
        std::fs::write(&agent_db, b"agent").unwrap();
        std::fs::write(&message_db, b"message").unwrap();

        let report = archive_runtime_agent(&config, &state, &runtime).unwrap();

        assert_eq!(report.moved_paths.len(), 2);
        assert!(!agent_db.exists());
        assert!(!message_db.exists());
        assert!(report.archive_dir.join("manifest.json").is_file());
        assert_eq!(
            state
                .load_agent_definition(&runtime.agent_did)
                .unwrap()
                .status,
            "archived"
        );
    }

    #[test]
    fn daemon_archive_finalizer_skips_service_uninstall_for_dev_state_root() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();
        std::fs::write(config.state_root.join("daemon.db"), b"state").unwrap();
        let archive_id = format!(
            "archive-dev-daemon-{}",
            root.path()
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("temp")
        );

        let report = finalize_daemon_archive(&config, &archive_id).unwrap();

        assert_eq!(
            report.archived_state_root,
            root.path()
                .parent()
                .unwrap()
                .join("archive")
                .join("daemon")
                .join(archive_id)
                .join("state")
        );
        assert!(report.state_root_moved);
        assert!(!report.service_uninstalled);
        assert!(report.archived_state_root.join("daemon.db").is_file());
    }

    #[test]
    fn custom_state_root_never_owns_product_service_without_a_default_root() {
        let custom_root = Path::new("/srv/awiki/custom-state");

        assert!(!state_root_owns_product_service(custom_root, None));
        assert!(!state_root_owns_product_service(
            custom_root,
            Some(Path::new("/home/alice/.awiki-daemon/deamon/state"))
        ));
        assert!(state_root_owns_product_service(
            custom_root,
            Some(custom_root)
        ));
    }

    #[test]
    fn pending_daemon_archive_finalizer_round_trips_marker() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();

        assert!(pending_daemon_archive_finalizer(&config).unwrap().is_none());

        write_pending_daemon_archive_finalizer(&config, "archive-daemon-1").unwrap();

        assert_eq!(
            pending_daemon_archive_finalizer(&config)
                .unwrap()
                .as_deref(),
            Some("archive-daemon-1")
        );
    }

    #[test]
    fn foreground_shutdown_finalizer_moves_dev_state_root() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();
        std::fs::write(config.state_root.join("daemon.db"), b"state").unwrap();
        let archive_id = format!(
            "foreground-shutdown-daemon-{}",
            root.path()
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("temp")
        );

        let report = finalize_daemon_archive_for_foreground_shutdown(&config, &archive_id).unwrap();

        assert_eq!(report.archive_id, archive_id);
        assert!(report.state_root_moved);
        assert!(!report.service_uninstalled);
        assert!(report.archived_state_root.join("daemon.db").is_file());
    }
}
