use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceBindingConfig {
    pub workspace_id: String,
    pub workspace_root: PathBuf,
    pub workspace_mode: WorkspaceMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceInstance {
    pub workspace_id: String,
    pub workspace_root: PathBuf,
    pub workspace_instance_path: PathBuf,
    pub workspace_mode: WorkspaceMode,
    pub is_security_boundary: bool,
    pub isolation_note: String,
    pub cleanup_policy: WorkspaceCleanupPolicy,
    pub base_ref: Option<String>,
    pub branch_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceCleanupPolicy {
    None,
    Preserve,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceMode {
    SharedRoot,
    RouteRoot,
    WorktreePerTask,
    Container,
    Sandbox,
}

impl WorkspaceBindingConfig {
    pub fn validate(&self) -> Result<()> {
        if self.workspace_id.trim().is_empty() {
            bail!("workspace_id must not be empty");
        }
        if self.workspace_root.as_os_str().is_empty() {
            bail!("workspace_root must not be empty");
        }
        Ok(())
    }
}

impl WorkspaceMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SharedRoot => "shared-root",
            Self::RouteRoot => "route-root",
            Self::WorktreePerTask => "worktree-per-task",
            Self::Container => "container",
            Self::Sandbox => "sandbox",
        }
    }

    pub fn parse(input: &str) -> Result<Self> {
        match input.trim() {
            "shared-root" => Ok(Self::SharedRoot),
            "route-root" => Ok(Self::RouteRoot),
            "worktree-per-task" => Ok(Self::WorktreePerTask),
            "container" => Ok(Self::Container),
            "sandbox" => Ok(Self::Sandbox),
            other => bail!("unsupported workspace mode: {other}"),
        }
    }

    pub fn is_security_boundary(self) -> bool {
        matches!(self, Self::Container | Self::Sandbox)
    }

    pub fn isolation_note(self) -> &'static str {
        match self {
            Self::SharedRoot => "不是硬隔离，只适合个人低风险或读任务",
            Self::RouteRoot => "按消息会话隔离上下文目录，不是安全边界",
            Self::WorktreePerTask => "只隔离代码变更，不防系统凭据读取",
            Self::Container => "可作为安全边界，依赖容器配置",
            Self::Sandbox => "可作为安全边界，依赖 sandbox profile",
        }
    }
}

pub fn prepare_workspace_instance(
    runtime_temp_dir: &Path,
    binding: &WorkspaceBindingConfig,
    run_id: &str,
) -> Result<WorkspaceInstance> {
    binding.validate()?;
    if run_id.trim().is_empty() {
        bail!("run_id must not be empty");
    }
    match binding.workspace_mode {
        WorkspaceMode::SharedRoot => prepare_shared_root(binding),
        WorkspaceMode::RouteRoot => prepare_route_root(binding, run_id),
        WorkspaceMode::WorktreePerTask => prepare_worktree(runtime_temp_dir, binding, run_id),
        WorkspaceMode::Container | WorkspaceMode::Sandbox => {
            bail!(
                "workspace mode {} is not implemented for generic-cli yet",
                binding.workspace_mode.as_str()
            )
        }
    }
}

fn prepare_route_root(
    binding: &WorkspaceBindingConfig,
    route_key_hash: &str,
) -> Result<WorkspaceInstance> {
    validate_route_key_hash_component(route_key_hash)?;
    let conversations_root = binding.workspace_root.join("conversations");
    std::fs::create_dir_all(&conversations_root).with_context(|| {
        format!(
            "create route workspace root {}",
            conversations_root.display()
        )
    })?;
    let workspace_root = binding.workspace_root.canonicalize().with_context(|| {
        format!(
            "canonicalize route workspace binding root {}",
            binding.workspace_root.display()
        )
    })?;
    let conversations_root = conversations_root.canonicalize().with_context(|| {
        format!(
            "canonicalize conversations root {}",
            conversations_root.display()
        )
    })?;
    ensure_path_under(&conversations_root, &workspace_root)?;
    let route_path = conversations_root.join(route_key_hash);
    std::fs::create_dir_all(&route_path)
        .with_context(|| format!("create route workspace {}", route_path.display()))?;
    let route_path = route_path
        .canonicalize()
        .with_context(|| format!("canonicalize route workspace {}", route_path.display()))?;
    ensure_path_under(&route_path, &workspace_root)?;
    Ok(WorkspaceInstance {
        workspace_id: binding.workspace_id.clone(),
        workspace_root,
        workspace_instance_path: route_path,
        workspace_mode: WorkspaceMode::RouteRoot,
        is_security_boundary: WorkspaceMode::RouteRoot.is_security_boundary(),
        isolation_note: WorkspaceMode::RouteRoot.isolation_note().to_string(),
        cleanup_policy: WorkspaceCleanupPolicy::None,
        base_ref: None,
        branch_name: None,
    })
}

fn prepare_shared_root(binding: &WorkspaceBindingConfig) -> Result<WorkspaceInstance> {
    std::fs::create_dir_all(&binding.workspace_root)
        .with_context(|| format!("create workspace root {}", binding.workspace_root.display()))?;
    let workspace_root = binding.workspace_root.canonicalize().with_context(|| {
        format!(
            "canonicalize workspace root {}",
            binding.workspace_root.display()
        )
    })?;
    Ok(WorkspaceInstance {
        workspace_id: binding.workspace_id.clone(),
        workspace_root: workspace_root.clone(),
        workspace_instance_path: workspace_root,
        workspace_mode: WorkspaceMode::SharedRoot,
        is_security_boundary: WorkspaceMode::SharedRoot.is_security_boundary(),
        isolation_note: WorkspaceMode::SharedRoot.isolation_note().to_string(),
        cleanup_policy: WorkspaceCleanupPolicy::None,
        base_ref: None,
        branch_name: None,
    })
}

fn prepare_worktree(
    runtime_temp_dir: &Path,
    binding: &WorkspaceBindingConfig,
    run_id: &str,
) -> Result<WorkspaceInstance> {
    let workspace_root = binding.workspace_root.canonicalize().with_context(|| {
        format!(
            "canonicalize workspace root {}",
            binding.workspace_root.display()
        )
    })?;
    ensure_git_worktree_root(&workspace_root)?;
    let worktrees_root = runtime_temp_dir
        .join("worktrees")
        .join(sanitize_path_component(&binding.workspace_id));
    std::fs::create_dir_all(&worktrees_root)
        .with_context(|| format!("create worktree root {}", worktrees_root.display()))?;
    let runtime_temp_root = runtime_temp_dir.canonicalize().with_context(|| {
        format!(
            "canonicalize runtime temp dir {}",
            runtime_temp_dir.display()
        )
    })?;
    let worktrees_root = worktrees_root
        .canonicalize()
        .with_context(|| format!("canonicalize worktree root {}", worktrees_root.display()))?;
    ensure_path_under(&worktrees_root, &runtime_temp_root)?;
    let worktree_path = worktrees_root.join(sanitize_path_component(run_id));
    if worktree_path.exists() {
        bail!(
            "workspace worktree path already exists: {}",
            worktree_path.display()
        );
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(&workspace_root)
        .args(["worktree", "add", "--detach"])
        .arg(&worktree_path)
        .arg("HEAD")
        .output()
        .with_context(|| format!("create git worktree {}", worktree_path.display()))?;
    if !output.status.success() {
        bail!(
            "git worktree add failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let workspace_instance_path = worktree_path
        .canonicalize()
        .with_context(|| format!("canonicalize worktree {}", worktree_path.display()))?;
    ensure_path_under(&workspace_instance_path, &runtime_temp_root)?;
    Ok(WorkspaceInstance {
        workspace_id: binding.workspace_id.clone(),
        workspace_root,
        workspace_instance_path,
        workspace_mode: WorkspaceMode::WorktreePerTask,
        is_security_boundary: WorkspaceMode::WorktreePerTask.is_security_boundary(),
        isolation_note: WorkspaceMode::WorktreePerTask.isolation_note().to_string(),
        cleanup_policy: WorkspaceCleanupPolicy::Preserve,
        base_ref: Some("HEAD".to_string()),
        branch_name: None,
    })
}

fn ensure_git_worktree_root(workspace_root: &Path) -> Result<()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .with_context(|| format!("inspect git workspace {}", workspace_root.display()))?;
    if !output.status.success() || String::from_utf8_lossy(&output.stdout).trim() != "true" {
        bail!(
            "workspace_root is not a git worktree: {}",
            workspace_root.display()
        );
    }
    Ok(())
}

fn ensure_path_under(path: &Path, parent: &Path) -> Result<()> {
    if !path.starts_with(parent) {
        bail!(
            "workspace path {} escapes runtime temp dir {}",
            path.display(),
            parent.display()
        );
    }
    Ok(())
}

fn validate_route_key_hash_component(value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("route workspace hash must not be empty");
    }
    if value != value.trim() || matches!(value, "." | "..") || value.contains("..") {
        bail!("route workspace hash contains unsupported path segments");
    }
    if value
        .bytes()
        .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')))
    {
        bail!("route workspace hash contains unsupported characters");
    }
    Ok(())
}

pub fn sanitize_path_component(input: &str) -> String {
    let sanitized = input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "workspace".to_string()
    } else {
        sanitized
    }
}
