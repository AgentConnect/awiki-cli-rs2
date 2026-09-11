use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};

pub mod deepseek_harness;

static ENTRIES: [&AcpAgentCatalogEntry; 1] = [&deepseek_harness::ENTRY];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpLaunchCwd {
    InstallRoot,
    ProfileDirectory,
    WorkspaceRoot,
}

pub struct AcpAgentCatalogEntry {
    pub agent_id: &'static str,
    pub display_name: &'static str,
    pub program_name: &'static str,
    pub minimum_program_version: Option<(u64, u64)>,
    pub default_npm_version: &'static str,
    pub npm_entrypoint: &'static str,
    pub local_entrypoint: &'static str,
    pub local_build_hint: &'static str,
    pub local_package_paths: &'static [(&'static str, &'static str)],
    pub credential_env_names: &'static [&'static str],
    pub required_credential_env_names: &'static [&'static str],
    pub inherited_process_env_names: &'static [&'static str],
    pub default_permission_policy: &'static str,
    pub npm_packages: fn(&str) -> Vec<String>,
    pub launch_cwd: AcpLaunchCwd,
    pub launch_args: fn(&Path, &Path) -> Vec<String>,
    pub render_config: fn(&Path, &Path) -> Result<String>,
}

impl AcpAgentCatalogEntry {
    pub fn launch_cwd(
        &self,
        install_root: &Path,
        profile_dir: &Path,
        workspace_root: &Path,
    ) -> PathBuf {
        match self.launch_cwd {
            AcpLaunchCwd::InstallRoot => install_root.to_path_buf(),
            AcpLaunchCwd::ProfileDirectory => profile_dir.to_path_buf(),
            AcpLaunchCwd::WorkspaceRoot => workspace_root.to_path_buf(),
        }
    }

    pub fn launch_args(&self, entrypoint: &Path, config_path: &Path) -> Vec<String> {
        (self.launch_args)(entrypoint, config_path)
    }

    pub fn render_config(&self, workspace_root: &Path, persistence_root: &Path) -> Result<String> {
        (self.render_config)(workspace_root, persistence_root)
    }

    fn validate(&self) -> Result<()> {
        for (field, value) in [
            ("agent_id", self.agent_id),
            ("display_name", self.display_name),
            ("program_name", self.program_name),
            ("default_npm_version", self.default_npm_version),
            ("npm_entrypoint", self.npm_entrypoint),
            ("local_entrypoint", self.local_entrypoint),
            ("local_build_hint", self.local_build_hint),
        ] {
            if value.trim().is_empty() {
                bail!("ACP catalog {field} must not be empty");
            }
        }
        for (field, value) in [
            ("npm_entrypoint", self.npm_entrypoint),
            ("local_entrypoint", self.local_entrypoint),
        ] {
            validate_relative_path(field, value)?;
        }
        for (link, target) in self.local_package_paths {
            validate_relative_path("local package link", link)?;
            validate_relative_path("local package target", target)?;
        }
        for required in self.required_credential_env_names {
            if !self.credential_env_names.contains(required) {
                bail!("ACP required credential must be present in credential_env_names");
            }
        }
        Ok(())
    }
}

pub fn entry(agent_id: &str) -> Result<&'static AcpAgentCatalogEntry> {
    find(agent_id)?.with_context(|| {
        format!(
            "unsupported ACP agent catalog entry: {}",
            agent_id.trim().to_ascii_lowercase()
        )
    })
}

pub fn find(agent_id: &str) -> Result<Option<&'static AcpAgentCatalogEntry>> {
    let normalized = agent_id.trim().to_ascii_lowercase();
    let entry = entries()
        .iter()
        .copied()
        .find(|entry| entry.agent_id == normalized);
    if let Some(entry) = entry {
        entry.validate()?;
    }
    Ok(entry)
}

pub fn entries() -> &'static [&'static AcpAgentCatalogEntry] {
    &ENTRIES
}

fn validate_relative_path(field: &str, value: &str) -> Result<()> {
    let path = Path::new(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("ACP catalog {field} must be a safe relative path");
    }
    Ok(())
}
