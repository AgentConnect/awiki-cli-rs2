use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::DaemonConfig;
use crate::runtime::RuntimeAgentProfile;
use crate::state::{DaemonState, HermesProfileRecord};

pub const HERMES_RUNTIME_NAME: &str = "hermes";
pub const HERMES_RUNTIME_PLUGIN_ID: &str = "runtime.hermes";

pub const AWIKI_SKILLS_VERSION: &str = "awiki-hermes-skills-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HermesProfileInstallResult {
    pub record: HermesProfileRecord,
    pub soul_path: PathBuf,
    pub profile_config_path: PathBuf,
    pub skill_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AwikiHermesProfileConfig {
    schema: String,
    agent_did: String,
    runtime_profile_id: String,
    controller_did: String,
    runtime_plugin_id: String,
    hermes_profile: String,
    local_rpc_socket_path: PathBuf,
    daemon_cli_wrapper: String,
    awiki_skills_version: String,
    run_capability_token_persisted: bool,
    notes: Vec<String>,
}

pub fn initialize_hermes_profile(
    config: &DaemonConfig,
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    handle: &str,
) -> Result<HermesProfileInstallResult> {
    if profile.runtime_plugin_id != HERMES_RUNTIME_PLUGIN_ID {
        bail!("Hermes profile initialization requires runtime.hermes");
    }
    let hermes_profile = hermes_profile_name(handle)?;
    let hermes_home = hermes_home(config, &profile.agent_did)?;
    ensure_child_path(&hermes_home, &config.state_root)?;
    std::fs::create_dir_all(&hermes_home)
        .with_context(|| format!("create Hermes profile home {}", hermes_home.display()))?;

    let soul_path = hermes_home.join("SOUL.md");
    let profile_config_path = hermes_home.join("awiki-profile.json");
    std::fs::write(&soul_path, soul_content(profile, &hermes_profile))
        .with_context(|| format!("write {}", soul_path.display()))?;
    let profile_config = AwikiHermesProfileConfig {
        schema: "awiki.hermes.profile.v1".to_string(),
        agent_did: profile.agent_did.clone(),
        runtime_profile_id: profile.runtime_profile_id.clone(),
        controller_did: profile.controller_did.clone(),
        runtime_plugin_id: profile.runtime_plugin_id.clone(),
        hermes_profile: hermes_profile.clone(),
        local_rpc_socket_path: config.local_socket_path.clone(),
        daemon_cli_wrapper: "library:awiki_deamon::cli_wrapper; process wrapper wired in Step 07"
            .to_string(),
        awiki_skills_version: AWIKI_SKILLS_VERSION.to_string(),
        run_capability_token_persisted: false,
        notes: vec![
            "run capability tokens are issued per message run and must not be persisted here"
                .to_string(),
            "DID private keys and user JWTs stay in daemon-managed storage".to_string(),
            "Hermes must call the daemon wrapper for Awiki capabilities".to_string(),
        ],
    };
    std::fs::write(
        &profile_config_path,
        serde_json::to_vec_pretty(&profile_config)?,
    )
    .with_context(|| format!("write {}", profile_config_path.display()))?;

    let skill_paths = install_skills(&hermes_home)?;
    smoke_check_profile(&hermes_home, &soul_path, &profile_config_path, &skill_paths)?;

    let record = HermesProfileRecord {
        agent_did: profile.agent_did.clone(),
        runtime_profile_id: profile.runtime_profile_id.clone(),
        hermes_profile,
        hermes_home,
        hermes_version: None,
        awiki_skills_version: AWIKI_SKILLS_VERSION.to_string(),
        status: "ready".to_string(),
    };
    state.upsert_hermes_profile(&record)?;
    Ok(HermesProfileInstallResult {
        record,
        soul_path,
        profile_config_path,
        skill_paths,
    })
}

pub fn mark_hermes_profile_failed(
    config: &DaemonConfig,
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    handle: &str,
) -> Result<()> {
    let record = HermesProfileRecord {
        agent_did: profile.agent_did.clone(),
        runtime_profile_id: profile.runtime_profile_id.clone(),
        hermes_profile: hermes_profile_name(handle)?,
        hermes_home: hermes_home(config, &profile.agent_did)?,
        hermes_version: None,
        awiki_skills_version: AWIKI_SKILLS_VERSION.to_string(),
        status: "failed".to_string(),
    };
    state.upsert_hermes_profile(&record)
}

pub fn hermes_home(config: &DaemonConfig, agent_did: &str) -> Result<PathBuf> {
    Ok(config
        .state_root
        .join("runtime")
        .join("hermes")
        .join("profiles")
        .join(stable_segment(agent_did)?))
}

pub fn hermes_profile_name(handle: &str) -> Result<String> {
    Ok(format!("awiki_{}", stable_segment(handle)?))
}

fn install_skills(hermes_home: &Path) -> Result<Vec<PathBuf>> {
    let skills_root = hermes_home.join("skills");
    let skills = [
        ("awiki-runtime", runtime_skill()),
        ("awiki-messaging", messaging_skill()),
        ("awiki-collaboration", collaboration_skill()),
    ];
    let mut paths = Vec::new();
    for (name, content) in skills {
        let path = skills_root.join(name).join("SKILL.md");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create Hermes Skill directory {}", parent.display()))?;
        }
        std::fs::write(&path, content).with_context(|| format!("write {}", path.display()))?;
        paths.push(path);
    }
    Ok(paths)
}

fn smoke_check_profile(
    hermes_home: &Path,
    soul_path: &Path,
    profile_config_path: &Path,
    skill_paths: &[PathBuf],
) -> Result<()> {
    if !hermes_home.is_dir() {
        bail!("Hermes profile home was not created");
    }
    ensure_non_empty_file(soul_path)?;
    ensure_non_empty_file(profile_config_path)?;
    for path in skill_paths {
        ensure_non_empty_file(path)?;
    }
    let forbidden_plugin = hermes_home.join("plugins").join("awiki-runtime");
    if forbidden_plugin.exists() {
        bail!("Hermes Python plugin directory must not be created");
    }
    Ok(())
}

fn ensure_non_empty_file(path: &Path) -> Result<()> {
    let metadata = std::fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    if !metadata.is_file() || metadata.len() == 0 {
        bail!(
            "Hermes profile file is missing or empty: {}",
            path.display()
        );
    }
    Ok(())
}

fn soul_content(profile: &RuntimeAgentProfile, hermes_profile: &str) -> String {
    format!(
        r#"# Awiki Hermes Agent

你正在作为 Awiki Hermes Runtime Agent 运行。所有 Awiki 能力都必须通过 daemon CLI wrapper 和 local RPC 完成。

- agent_did: {agent_did}
- runtime_profile_id: {runtime_profile_id}
- runtime_plugin_id: {runtime_plugin_id}
- controller_did: {controller_did}
- hermes_profile: {hermes_profile}

边界：

- 不直接连接 message-service。
- 不读取或持有 DID 私钥。
- 不持久化 run capability token。
- 不安装或依赖 Hermes Python plugin。
- 对 controller message 使用 message/run 语义，不引入 product task workflow。
"#,
        agent_did = profile.agent_did,
        runtime_profile_id = profile.runtime_profile_id,
        runtime_plugin_id = profile.runtime_plugin_id,
        controller_did = profile.controller_did,
        hermes_profile = hermes_profile,
    )
}

fn runtime_skill() -> &'static str {
    r#"# Awiki Runtime

当你处理由 daemon 校验后的 controller message 时，使用 message/run 语义描述进度。

- 需要报告进度时，调用 daemon wrapper 的 `report-status` 能力。
- 完成回复时，调用 daemon wrapper 的 `finish-message` 能力。
- 首版失败结果使用 failed status；不要把失败包装成 success final。
- local RPC 当前兼容方法名仍可能是 `task.status` / `task.finish`，这不代表产品层 task workflow。
- run token 只由 daemon 在本次 message run 前注入，不得写入 profile 或日志。
"#
}

fn messaging_skill() -> &'static str {
    r#"# Awiki Messaging

当你需要联系 human 或其他 agent 时，必须通过 daemon wrapper 的 `send-message` 能力。

- daemon 会校验 run token、method scope 和 recipient scope。
- 只有 wrapper 返回成功后，才可以声称消息已经发送。
- 不直接连接 message-service。
- 不伪造 DID，不读取 DID 私钥。
- `msg.send` 的目标语义是真实 ANP direct/direct-e2ee 外发消息，不是状态消息。
"#
}

fn collaboration_skill() -> &'static str {
    r#"# Awiki Collaboration

agent-to-agent 协作仍然通过 Awiki messaging 能力完成。

- 非 controller 消息不自动进入执行链。
- 需要协作时，通过 `awiki-messaging` 的 `send-message` 发送给目标 DID。
- 不读取 inbox、conversation history 或 handle resolver；这些能力不属于 Hermes MVP。
"#
}

fn stable_segment(input: &str) -> Result<String> {
    let value = input
        .trim()
        .trim_start_matches('@')
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
        .to_string();
    if value.is_empty() {
        bail!("Hermes profile segment source must not be empty");
    }
    Ok(value)
}

fn ensure_child_path(path: &Path, root: &Path) -> Result<()> {
    let path = path
        .canonicalize()
        .or_else(|_| {
            path.parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| path.to_path_buf())
                .canonicalize()
        })
        .unwrap_or_else(|_| path.to_path_buf());
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    if !path.starts_with(&root) {
        bail!("Hermes profile path must stay under daemon state_root");
    }
    Ok(())
}
