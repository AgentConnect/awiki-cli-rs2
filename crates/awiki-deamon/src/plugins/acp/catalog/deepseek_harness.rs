use std::path::Path;

use anyhow::{Context, Result};

use super::{AcpAgentCatalogEntry, AcpLaunchCwd};

pub const DEEPSEEK_HARNESS_AGENT_ID: &str = "deepseek-harness";
pub const DEEPSEEK_HARNESS_NPM_VERSION: &str = "0.1.0-rc.6";
pub const DEEPSEEK_HARNESS_LOCAL_ENTRYPOINT: &str = "packages/examples/acp-demo/lib/bin.js";

const NPM_PACKAGE_NAMES: &[&str] = &[
    "@deepseek-ai/dsh-acp-demo",
    "@deepseek-ai/dsh-llm-deepseek",
    "@deepseek-ai/dsh-sandbox-local",
    "@deepseek-ai/dsh-sandbox-policy",
    "@deepseek-ai/dsh-subprocess-local",
    "@deepseek-ai/dsh-bash-sandbox",
    "@deepseek-ai/dsh-user-approval",
    "@deepseek-ai/dsh-fs-sandbox",
    "@deepseek-ai/dsh-fs-observation-policy",
    "@deepseek-ai/dsh-tool-fs",
    "@deepseek-ai/dsh-tool-bash",
];

pub static ENTRY: AcpAgentCatalogEntry = AcpAgentCatalogEntry {
    agent_id: DEEPSEEK_HARNESS_AGENT_ID,
    display_name: "DeepSeek Harness",
    program_name: "node",
    minimum_program_version: Some((22, 19)),
    default_npm_version: DEEPSEEK_HARNESS_NPM_VERSION,
    npm_entrypoint: "node_modules/@deepseek-ai/dsh-acp-demo/lib/bin.js",
    local_entrypoint: DEEPSEEK_HARNESS_LOCAL_ENTRYPOINT,
    local_build_hint: "run pnpm install && pnpm run build first",
    local_package_paths: &[
        ("@deepseek-ai/dsh-acp-demo", "packages/examples/acp-demo"),
        ("@deepseek-ai/dsh-llm-deepseek", "packages/llm/llm-deepseek"),
        (
            "@deepseek-ai/dsh-sandbox-local",
            "packages/sandbox/sandbox-local",
        ),
        (
            "@deepseek-ai/dsh-sandbox-policy",
            "packages/sandbox/sandbox-policy",
        ),
        (
            "@deepseek-ai/dsh-subprocess-local",
            "packages/subprocess/subprocess-local",
        ),
        (
            "@deepseek-ai/dsh-bash-sandbox",
            "packages/shell/bash-sandbox",
        ),
        (
            "@deepseek-ai/dsh-user-approval",
            "packages/interaction/user-approval",
        ),
        ("@deepseek-ai/dsh-fs-sandbox", "packages/fs/fs-sandbox"),
        (
            "@deepseek-ai/dsh-fs-observation-policy",
            "packages/fs/fs-observation-policy",
        ),
        ("@deepseek-ai/dsh-tool-fs", "packages/fs/tool-fs"),
        ("@deepseek-ai/dsh-tool-bash", "packages/shell/tool-bash"),
    ],
    credential_env_names: &["DEEPSEEK_API_KEY", "DEEPSEEK_BASE_URL"],
    required_credential_env_names: &["DEEPSEEK_API_KEY"],
    inherited_process_env_names: &[
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "no_proxy",
        "LANG",
        "LC_ALL",
        "TMPDIR",
    ],
    default_permission_policy: "allow-once",
    npm_packages: deepseek_harness_npm_packages,
    launch_cwd: AcpLaunchCwd::InstallRoot,
    launch_args: deepseek_harness_launch_args,
    render_config: render_deepseek_harness_config,
};

pub fn deepseek_harness_npm_packages(version: &str) -> Vec<String> {
    NPM_PACKAGE_NAMES
        .iter()
        .map(|name| format!("{name}@{version}"))
        .collect()
}

fn deepseek_harness_launch_args(entrypoint: &Path, config_path: &Path) -> Vec<String> {
    vec![
        entrypoint.display().to_string(),
        "--config".to_string(),
        config_path.display().to_string(),
    ]
}

fn render_deepseek_harness_config(
    workspace_root: &Path,
    persistence_root: &Path,
) -> Result<String> {
    let workspace = serde_json::to_string(
        workspace_root
            .to_str()
            .context("ACP workspace path must be valid UTF-8")?,
    )?;
    let persistence = serde_json::to_string(
        persistence_root
            .to_str()
            .context("ACP persistence path must be valid UTF-8")?,
    )?;
    Ok(format!(
        r#"- id: llm-deepseek
  name: '@deepseek-ai/dsh-llm-deepseek'
  config:
    thinking: enabled
    reasoningEffort: max
    models:
      - id: deepseek-v4-pro

- id: sandbox
  name: '@deepseek-ai/dsh-sandbox-local'

- id: sandbox-policy
  name: '@deepseek-ai/dsh-sandbox-policy'
  config:
    mode: workspace-write
    workspaceRoot: {workspace}

- id: subprocess
  name: '@deepseek-ai/dsh-subprocess-local'

- id: bash
  name: '@deepseek-ai/dsh-bash-sandbox'
  config:
    timeoutMs: 60000

- id: approval
  name: '@deepseek-ai/dsh-user-approval'
  config:
    policy: ask

- id: fs-sandbox
  name: '@deepseek-ai/dsh-fs-sandbox'
  config:
    cwd: {workspace}

- id: fs-observation-policy
  name: '@deepseek-ai/dsh-fs-observation-policy'

- id: tool-fs
  name: '@deepseek-ai/dsh-tool-fs'

- id: tool-bash
  name: '@deepseek-ai/dsh-tool-bash'

- id: acp-agent
  name: '@deepseek-ai/dsh-acp-demo'
  config:
    provider: deepseek-official
    model: deepseek-v4-pro
    persistenceRoot: {persistence}
    workspaceContext:
      maxBytes: 65536
    persona: |
      You are a coding assistant. Your working directory is {{{{cwd}}}}.
      Verify work with relevant tests and keep answers brief and factual.
"#
    ))
}
