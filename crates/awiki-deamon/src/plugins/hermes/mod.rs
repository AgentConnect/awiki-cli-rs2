use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::DaemonConfig;
use crate::runtime::RuntimeAgentProfile;
use crate::state::{DaemonState, HermesProfileRecord};

pub mod gateway;
pub mod prompt;
pub mod runner;

pub use gateway::{
    FakeHermesBehavior, FakeHermesGateway, HermesGateway, HermesGatewayCommandStatus,
    HermesGatewayLaunchContext, HermesGatewayTimeouts, HermesPromptOutcome,
    HermesPromptSubmitRequest, HermesRunnerRef, HermesRuntimeEvent, HermesRuntimeEventKind,
    HermesSessionCreateRequest, HermesSessionRef, HermesSessionResumeRequest, StdioHermesGateway,
};
pub use prompt::HermesPromptWrapper;
pub use runner::{reset_hermes_session_by_route, HermesRunner, HermesRuntimePlugin};

pub const HERMES_RUNTIME_NAME: &str = "hermes";
pub const HERMES_RUNTIME_PLUGIN_ID: &str = "runtime.hermes";

pub const AWIKI_SKILLS_VERSION: &str = "awiki-hermes-skills-v3";
const HERMES_BASE_CONFIG_PATH_ENV: &str = "AWIKI_HERMES_BASE_CONFIG_PATH";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HermesProfileInstallResult {
    pub record: HermesProfileRecord,
    pub soul_path: PathBuf,
    pub profile_config_path: PathBuf,
    pub model_config_path: Option<PathBuf>,
    pub dotenv_path: Option<PathBuf>,
    pub skill_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AwikiHermesProfileConfig {
    schema: String,
    agent_did: String,
    runtime_profile_id: String,
    controller_did: String,
    runtime_plugin_id: String,
    preferred_language: String,
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
        preferred_language: profile.preferred_language.clone(),
        hermes_profile: hermes_profile.clone(),
        local_rpc_socket_path: config.local_socket_path.clone(),
        daemon_cli_wrapper: "process:awiki-deamon-runtime via daemon-managed Hermes PATH"
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

    let runtime_config = ensure_runtime_model_config(&hermes_home)?;
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
        model_config_path: runtime_config.model_config_path,
        dotenv_path: runtime_config.dotenv_path,
        skill_paths,
    })
}

pub fn repair_hermes_profile_if_needed(
    config: &DaemonConfig,
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    handle: &str,
) -> Result<Option<HermesProfileInstallResult>> {
    if profile.runtime_plugin_id != HERMES_RUNTIME_PLUGIN_ID {
        return Ok(None);
    }
    let record = match state.load_hermes_profile(&profile.agent_did) {
        Ok(record) => record,
        Err(_) => return initialize_hermes_profile(config, state, profile, handle).map(Some),
    };
    if record.status == "ready"
        && record.awiki_skills_version == AWIKI_SKILLS_VERSION
        && hermes_profile_files_are_current(&record)
    {
        return Ok(None);
    }
    rewrite_existing_hermes_profile(config, state, profile, handle, record).map(Some)
}

fn hermes_profile_files_are_current(record: &HermesProfileRecord) -> bool {
    let skill_path = record
        .hermes_home
        .join("skills")
        .join("awiki-outbound-messaging")
        .join("SKILL.md");
    let Ok(skill) = std::fs::read_to_string(skill_path) else {
        return false;
    };
    skill.contains("awiki-deamon-runtime send")
        && skill.contains("--to <handle-or-did>")
        && skill.contains("--group")
        && skill.contains("Do not call `awiki-cli`")
        && !skill.contains("--to-handle")
}

fn rewrite_existing_hermes_profile(
    config: &DaemonConfig,
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    handle: &str,
    mut record: HermesProfileRecord,
) -> Result<HermesProfileInstallResult> {
    ensure_child_path(&record.hermes_home, &config.state_root)?;
    std::fs::create_dir_all(&record.hermes_home).with_context(|| {
        format!(
            "create Hermes profile home {}",
            record.hermes_home.display()
        )
    })?;
    let expected_hermes_profile = hermes_profile_name(handle)?;
    if record.hermes_profile.trim().is_empty() {
        record.hermes_profile = expected_hermes_profile;
    }

    let soul_path = record.hermes_home.join("SOUL.md");
    let profile_config_path = record.hermes_home.join("awiki-profile.json");
    std::fs::write(&soul_path, soul_content(profile, &record.hermes_profile))
        .with_context(|| format!("write {}", soul_path.display()))?;
    let profile_config = AwikiHermesProfileConfig {
        schema: "awiki.hermes.profile.v1".to_string(),
        agent_did: profile.agent_did.clone(),
        runtime_profile_id: profile.runtime_profile_id.clone(),
        controller_did: profile.controller_did.clone(),
        runtime_plugin_id: profile.runtime_plugin_id.clone(),
        preferred_language: profile.preferred_language.clone(),
        hermes_profile: record.hermes_profile.clone(),
        local_rpc_socket_path: config.local_socket_path.clone(),
        daemon_cli_wrapper: "process:awiki-deamon-runtime via daemon-managed Hermes PATH"
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

    let runtime_config = ensure_runtime_model_config(&record.hermes_home)?;
    let skill_paths = install_skills(&record.hermes_home)?;
    smoke_check_profile(
        &record.hermes_home,
        &soul_path,
        &profile_config_path,
        &skill_paths,
    )?;

    record.runtime_profile_id = profile.runtime_profile_id.clone();
    record.awiki_skills_version = AWIKI_SKILLS_VERSION.to_string();
    record.status = "ready".to_string();
    state.upsert_hermes_profile(&record)?;
    Ok(HermesProfileInstallResult {
        record,
        soul_path,
        profile_config_path,
        model_config_path: runtime_config.model_config_path,
        dotenv_path: runtime_config.dotenv_path,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HermesRuntimeConfigProvisionResult {
    pub model_config_path: Option<PathBuf>,
    pub dotenv_path: Option<PathBuf>,
}

pub fn ensure_runtime_model_config(
    hermes_home: &Path,
) -> Result<HermesRuntimeConfigProvisionResult> {
    std::fs::create_dir_all(hermes_home)
        .with_context(|| format!("create Hermes profile home {}", hermes_home.display()))?;

    let model_config_path = copy_runtime_config_file(
        &base_hermes_config_path(),
        &hermes_home.join("config.yaml"),
        true,
    )?;
    let dotenv_path =
        copy_runtime_config_file(&base_hermes_dotenv_path(), &hermes_home.join(".env"), false)?;
    Ok(HermesRuntimeConfigProvisionResult {
        model_config_path,
        dotenv_path,
    })
}

pub fn hermes_runtime_model_config_status(hermes_home: &Path) -> HermesRuntimeModelConfigStatus {
    let config_path = hermes_home.join("config.yaml");
    if ensure_non_empty_file(&config_path).is_ok() {
        HermesRuntimeModelConfigStatus::Configured
    } else if base_hermes_config_path().is_some() {
        HermesRuntimeModelConfigStatus::Repairable
    } else {
        HermesRuntimeModelConfigStatus::MissingBaseConfig
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HermesRuntimeModelConfigStatus {
    Configured,
    Repairable,
    MissingBaseConfig,
}

impl HermesRuntimeModelConfigStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Configured => "configured",
            Self::Repairable => "repairable",
            Self::MissingBaseConfig => "missing_base_config",
        }
    }

    pub fn needs_config(self) -> bool {
        self != Self::Configured
    }

    pub fn error_code(self) -> Option<&'static str> {
        match self {
            Self::Configured => None,
            Self::Repairable => Some("hermes_model_config_repairable"),
            Self::MissingBaseConfig => Some("hermes_model_config_missing"),
        }
    }
}

fn base_hermes_config_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(HERMES_BASE_CONFIG_PATH_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| non_empty_file(path))
    {
        return Some(path);
    }
    default_hermes_home()
        .map(|home| home.join("config.yaml"))
        .filter(|path| non_empty_file(path))
}

fn base_hermes_dotenv_path() -> Option<PathBuf> {
    base_hermes_config_path()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .map(|home| home.join(".env"))
        .filter(|path| non_empty_file(path))
}

fn default_hermes_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|home| home.join(".hermes"))
}

fn copy_runtime_config_file(
    source: &Option<PathBuf>,
    destination: &Path,
    required: bool,
) -> Result<Option<PathBuf>> {
    if non_empty_file(destination) {
        return Ok(Some(destination.to_path_buf()));
    }
    let Some(source) = source else {
        return Ok(None);
    };
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create Hermes config directory {}", parent.display()))?;
    }
    std::fs::copy(source, destination).with_context(|| {
        format!(
            "copy Hermes runtime config {} -> {}",
            source.display(),
            destination.display()
        )
    })?;
    if required {
        ensure_non_empty_file(destination)?;
    }
    Ok(Some(destination.to_path_buf()))
}

fn non_empty_file(path: &Path) -> bool {
    path.metadata()
        .map(|metadata| metadata.is_file() && metadata.len() > 0)
        .unwrap_or(false)
}

fn install_skills(hermes_home: &Path) -> Result<Vec<PathBuf>> {
    let skills_root = hermes_home.join("skills");
    for legacy in ["awiki-runtime", "awiki-messaging", "awiki-collaboration"] {
        let legacy_path = skills_root.join(legacy);
        if legacy_path.exists() {
            std::fs::remove_dir_all(&legacy_path)
                .with_context(|| format!("remove legacy Hermes Skill {}", legacy_path.display()))?;
        }
    }
    let skills = [("awiki-outbound-messaging", outbound_messaging_skill())];
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
- preferred_language: {preferred_language}

边界：

- 不直接连接 message-service。
- 不读取或持有 DID 私钥。
- 不持久化 run capability token。
- 不安装或依赖 Hermes Python plugin。
- 对 controller message 使用 message/run 语义，不引入 product task workflow。
- 回复 controller 时始终跟随 controller 的会话语言；如果当前消息只有附件或无法判断语言，使用 preferred_language，不要因为系统 wrapper 使用英文标签就改用英文。
- preferred_language=en 表示英文；preferred_language=zh-Hans 表示简体中文。
"#,
        agent_did = profile.agent_did,
        runtime_profile_id = profile.runtime_profile_id,
        runtime_plugin_id = profile.runtime_plugin_id,
        controller_did = profile.controller_did,
        hermes_profile = hermes_profile,
        preferred_language = profile.preferred_language,
    )
}

fn outbound_messaging_skill() -> &'static str {
    r#"# Awiki Outbound Messaging

Use this Skill only when the controller explicitly asks you to send a separate message to another human handle or group. Do not use it for your ordinary final answer to the controller; daemon automatically sends Hermes final output back to the APP as the Runtime Agent.

Supported outbound sends:

- Direct text to a human or agent: `awiki-deamon-runtime send --to <handle-or-did> --text <text>`
- Direct attachment with caption to a human or agent: `awiki-deamon-runtime send --to <handle-or-did> --text <caption> --file <path> --display-filename <name> --mime-type <mime>`
- Group text: `awiki-deamon-runtime send --group <group_did_or_id> --text <text>`
- Group attachment with caption: `awiki-deamon-runtime send --group <group_did_or_id> --text <caption> --file <path> --display-filename <name> --mime-type <mime>`

Rules:

- Use `--to` for one direct human or agent recipient. The value may be a handle or a DID.
- Use an existing group DID or group id with `--group`.
- Use exactly one target: either `--to` or `--group`.
- Always use `awiki-deamon-runtime send` for outbound messaging. Do not call `awiki-cli`, do not change CLI profiles, and do not switch local identities.
- The daemon chooses the Runtime Agent as the sender. Never add, infer, or override a sender identity.
- All outbound sends are ordinary Awiki messages.
- For attachment sends, put the user-visible message in `--text`; it becomes the attachment caption in the same outbound message.
- Only say the outbound message was sent after the wrapper returns success.
- If the wrapper reports a recipient, membership, or auth failure, explain that failure to the controller. Do not retry with another local identity.
- Do not include tokens, socket paths, private keys, API keys, auth caches, or local log paths in outbound message text, captions, filenames, or visible status.
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
    let path = canonicalize_existing_prefix(path);
    let root = canonicalize_existing_prefix(root);
    if !path.starts_with(&root) {
        bail!("Hermes profile path must stay under daemon state_root");
    }
    Ok(())
}

fn canonicalize_existing_prefix(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    let Some(parent) = path.parent() else {
        return path.to_path_buf();
    };
    let canonical_parent = if parent == path {
        parent.to_path_buf()
    } else {
        canonicalize_existing_prefix(parent)
    };
    match path.file_name() {
        Some(file_name) => canonical_parent.join(file_name),
        None => canonical_parent,
    }
}
