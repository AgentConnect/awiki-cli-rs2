//! Versioned, private adapter components distributed with the daemon binary.
//! The manifest is shared with the package builder; the host's Agent executable
//! is passed through the upstream adapter's supported override.
use super::Brand;
use agent_client_protocol::AcpAgentConfig;
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

const SPECIFICATION: &str = include_str!("../../../../scripts/release/daemon/acp/components.json");

pub(crate) struct Adapter {
    pub node: PathBuf,
    pub entry: PathBuf,
    pub version: String,
    pub node_version: String,
    executable_env: String,
}

pub(crate) fn platform() -> String {
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else {
        std::env::consts::OS
    };
    let arch = if cfg!(target_arch = "x86_64") {
        "amd64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        std::env::consts::ARCH
    };
    format!("{os}-{arch}")
}

impl Adapter {
    pub(crate) fn discover(brand: Brand) -> Result<Self> {
        // Hermetic source-build tests can supply a private fixture package.
        // The normal manifest/path checks still apply; release artifacts never
        // accept this test override or search the checkout.
        #[cfg(debug_assertions)]
        if let Some(root) = std::env::var_os("AWIKI_ACP_TEST_COMPONENTS_DIR") {
            return Self::from_directory(Path::new(&root), brand, &platform());
        }
        let executable = std::fs::canonicalize(std::env::current_exe()?)?;
        let packaged = executable
            .parent()
            .context("daemon_directory_missing")?
            .join("acp");
        if packaged.exists() {
            return Self::from_directory(&packaged, brand, &platform());
        }
        // Source builds use components prepared explicitly in this repository.
        // Release binaries never reach into a build machine's source directory.
        #[cfg(debug_assertions)]
        {
            let local = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/acp-components")
                .join(platform());
            if local.exists() {
                return Self::from_directory(&local, brand, &platform());
            }
        }
        bail!("acp_adapter_missing")
    }

    pub(crate) fn from_directory(root: &Path, brand: Brand, platform: &str) -> Result<Self> {
        let spec: Value = serde_json::from_str(SPECIFICATION)?;
        if spec["node_archives"].get(platform).is_none() {
            bail!("acp_adapter_platform_unsupported");
        }
        let configured = &spec["adapters"][brand.id()];
        if !configured.is_object() {
            bail!("acp_adapter_not_required");
        }
        let bytes =
            std::fs::read(root.join("manifest.json")).context("acp_adapter_manifest_missing")?;
        let manifest: Value =
            serde_json::from_slice(&bytes).context("acp_adapter_manifest_invalid")?;
        if manifest["schema_version"] != 1
            || manifest["platform"] != platform
            || manifest["available"] != true
            || manifest["node_version"] != spec["node_version"]
            || manifest["adapters"][brand.id()] != *configured
        {
            bail!("acp_adapter_incompatible");
        }
        let node = root.join("node");
        let entry = root.join(
            configured["entry"]
                .as_str()
                .context("acp_adapter_entry_missing")?,
        );
        // Full hashes are checked at installation, not on every conversation.
        // Neither executable may escape the versioned component directory.
        let canonical_root = root.canonicalize()?;
        for path in [&node, &entry] {
            let metadata = std::fs::symlink_metadata(path).context("acp_adapter_file_missing")?;
            if !metadata.is_file() || !path.canonicalize()?.starts_with(&canonical_root) {
                bail!("acp_adapter_file_invalid");
            }
        }
        Ok(Self {
            node,
            entry,
            version: configured["version"]
                .as_str()
                .context("acp_adapter_version_missing")?
                .into(),
            node_version: spec["node_version"]
                .as_str()
                .context("acp_node_version_missing")?
                .into(),
            executable_env: configured["executable_env"]
                .as_str()
                .context("acp_adapter_override_missing")?
                .into(),
        })
    }

    pub(crate) fn launch(&self, client: &Path) -> AcpAgentConfig {
        AcpAgentConfig::new(&self.node)
            .arg(self.entry.to_string_lossy())
            .env(&self.executable_env, client.to_string_lossy())
    }
}

#[cfg(test)]
#[path = "components_tests.rs"]
mod tests;
