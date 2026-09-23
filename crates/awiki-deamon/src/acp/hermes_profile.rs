//! First-use configuration seed for an isolated Hermes native home.
//! Never clone profiles: upstream clones also carry memories and user data.
use crate::{state::CliRuntimeProfileRecord, DaemonState};
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MODEL_FIELDS: &[&str] = &[
    "default",
    "model",
    "provider",
    "base_url",
    "api_key",
    "api_mode",
    "key_env",
    "context_length",
    "max_tokens",
];
const PROVIDER_FIELDS: &[&str] = &[
    "name",
    "base_url",
    "api_key",
    "api_mode",
    "key_env",
    "model",
    "context_length",
];
const STATIC_ENV: &[&str] = &[
    "OPENAI_API_KEY",
    "OPENAI_BASE_URL",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_BASE_URL",
    "OPENROUTER_API_KEY",
    "DEEPSEEK_API_KEY",
    "DEEPSEEK_BASE_URL",
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "GROQ_API_KEY",
    "MISTRAL_API_KEY",
    "TOGETHER_API_KEY",
    "FIREWORKS_API_KEY",
    "XAI_API_KEY",
    "CEREBRAS_API_KEY",
    "KIMI_API_KEY",
    "MOONSHOT_API_KEY",
    "ZAI_API_KEY",
    "GLM_API_KEY",
    "MINIMAX_API_KEY",
];

pub(crate) fn for_session(
    state: &DaemonState,
    profile: &CliRuntimeProfileRecord,
    key: &str,
) -> Result<CliRuntimeProfileRecord> {
    if profile.driver_id != "hermes" {
        return Ok(profile.clone());
    }
    let root = state
        .database_path()
        .parent()
        .context("daemon_state_root_missing")?
        .join("acp-hermes");
    // The ACP key already includes agent, owner and canonical conversation scope.
    let home = root.join(format!("{:x}", Sha256::digest(key.as_bytes())));
    let source = profile
        .config_home
        .clone()
        .or_else(|| {
            std::env::var_os("HERMES_HOME")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| awiki_user_dirs::try_home_dir().map(|p| p.join(".hermes")));
    // An explicit source home is self-contained. It must not borrow secrets
    // from a different ambient profile (including hermetic test fixtures).
    let environment = if profile.config_home.is_some() {
        Default::default()
    } else {
        std::env::vars().collect()
    };
    initialize(&home, source.as_deref(), &environment)?;
    let mut result = profile.clone();
    result.config_home = Some(home);
    Ok(result)
}

fn bounded_file(path: &Path) -> Result<Option<String>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => bail!("hermes_seed_unreadable"),
    };
    ensure!(file.metadata()?.is_file(), "hermes_seed_not_regular_file");
    let mut text = String::new();
    file.take(1024 * 1024 + 1)
        .read_to_string(&mut text)
        .map_err(|_| anyhow::anyhow!("hermes_seed_invalid_encoding"))?;
    ensure!(text.len() <= 1024 * 1024, "hermes_seed_too_large");
    Ok(Some(text))
}

fn scalar_fields(value: &Value, fields: &[&str], keys: &mut BTreeSet<String>) -> Value {
    let mut output = Map::new();
    for name in fields {
        if let Some(value) = value
            .get(*name)
            .filter(|v| v.is_string() || v.is_number() || v.is_boolean())
        {
            if *name == "key_env" {
                let Some(key) = value
                    .as_str()
                    .filter(|k| valid_key(k) && k.ends_with("_API_KEY"))
                else {
                    continue;
                };
                keys.insert(key.into());
            }
            output.insert((*name).into(), value.clone());
        }
    }
    Value::Object(output)
}

fn seed_config(raw: Option<&str>) -> Result<(Value, BTreeSet<String>)> {
    // Parse errors deliberately exclude the YAML excerpt: it may contain keys.
    let config: Value = match raw {
        Some(raw) => serde_yaml_ng::from_str(raw)
            .map_err(|_| anyhow::anyhow!("hermes_seed_invalid_config"))?,
        None => json!({}),
    };
    ensure!(
        config.is_object() || config.is_null(),
        "hermes_seed_invalid_config"
    );
    let mut keys: BTreeSet<String> = STATIC_ENV.iter().map(|v| (*v).into()).collect();
    let mut output = Map::new();
    if let Some(model) = config.get("model") {
        output.insert(
            "model".into(),
            if model.is_string() {
                model.clone()
            } else {
                scalar_fields(model, MODEL_FIELDS, &mut keys)
            },
        );
    }
    if let Some(providers) = config["providers"].as_object() {
        output.insert(
            "providers".into(),
            Value::Object(
                providers
                    .iter()
                    .map(|(k, v)| (k.clone(), scalar_fields(v, PROVIDER_FIELDS, &mut keys)))
                    .collect(),
            ),
        );
    }
    if let Some(providers) = config["custom_providers"].as_array() {
        output.insert(
            "custom_providers".into(),
            Value::Array(
                providers
                    .iter()
                    .map(|v| scalar_fields(v, PROVIDER_FIELDS, &mut keys))
                    .collect(),
            ),
        );
    }
    // Built-in memory keeps its upstream defaults; external provider, plugins,
    // MCP, hooks, skills, history, OAuth and arbitrary paths are never seeded.
    Ok((Value::Object(output), keys))
}

fn valid_key(key: &str) -> bool {
    key.len() <= 128
        && key
            .as_bytes()
            .first()
            .is_some_and(|c| c.is_ascii_uppercase() || *c == b'_')
        && key
            .bytes()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
}

fn seed_env(raw: &str, allowed: &BTreeSet<String>) -> Result<String> {
    let mut output = String::new();
    for line in raw.lines().map(str::trim) {
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if !allowed.contains(key) {
            continue;
        }
        let value = value.trim();
        // Seed literal one-line dotenv values only; never expand the parent
        // environment or evaluate shell syntax while copying credentials.
        ensure!(
            !value.contains("${") && !value.contains('\0'),
            "hermes_seed_env_requires_literal_value"
        );
        if value.starts_with('\'') || value.starts_with('"') {
            let quote = value.as_bytes()[0];
            ensure!(
                value.len() >= 2
                    && value.as_bytes().last() == Some(&quote)
                    && !value[1..value.len() - 1].contains(quote as char),
                "hermes_seed_env_requires_literal_value"
            );
        }
        output.push_str(key);
        output.push('=');
        output.push_str(value);
        output.push('\n');
    }
    Ok(output)
}

fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    ensure!(
        fs::symlink_metadata(path)?.is_dir(),
        "hermes_profile_invalid_directory"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn private_file(path: &Path, contents: &[u8]) -> Result<()> {
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(contents)?;
    file.sync_all()?;
    Ok(())
}

fn initialized(home: &Path) -> Result<bool> {
    match fs::symlink_metadata(home) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
        Ok(metadata) => {
            ensure!(metadata.is_dir(), "hermes_profile_invalid_directory");
            let marker = bounded_file(&home.join(".awiki-acp-profile.json"))?
                .context("hermes_profile_initialization_incomplete")?;
            let marker: Value = serde_json::from_str(&marker)
                .map_err(|_| anyhow::anyhow!("hermes_profile_marker_invalid"))?;
            ensure!(
                marker["schema_version"] == 1,
                "hermes_profile_marker_invalid"
            );
            Ok(true)
        }
    }
}

fn initialize(
    home: &Path,
    source: Option<&Path>,
    ambient: &std::collections::BTreeMap<String, String>,
) -> Result<()> {
    if initialized(home)? {
        return Ok(());
    }
    let parent = home.parent().context("hermes_profile_parent_missing")?;
    private_dir(parent)?;
    let stage = tempfile::Builder::new()
        .prefix(".prepare-")
        .tempdir_in(parent)?;
    private_dir(stage.path())?;
    let raw = source
        .map(|p| bounded_file(&p.join("config.yaml")))
        .transpose()?
        .flatten();
    let (config, keys) = seed_config(raw.as_deref())?;
    let raw_env = source
        .map(|p| bounded_file(&p.join(".env")))
        .transpose()?
        .flatten()
        .unwrap_or_default();
    let mut env = seed_env(&raw_env, &keys)?;
    let configured: BTreeSet<String> = env
        .lines()
        .filter_map(|line| line.split_once('=').map(|(k, _)| k.into()))
        .collect();
    for key in keys.difference(&configured) {
        if let Some(value) = ambient.get(key).filter(|v| !v.is_empty()) {
            ensure!(
                !value.contains(['\n', '\r', '\0']) && !value.contains("${"),
                "hermes_seed_env_requires_literal_value"
            );
            env.push_str(&format!("{key}={}\n", serde_json::to_string(value)?));
        }
    }
    // JSON is a YAML subset and avoids hand-written YAML quoting of secrets.
    private_file(
        &stage.path().join("config.yaml"),
        &serde_json::to_vec_pretty(&config)?,
    )?;
    private_file(&stage.path().join(".env"), env.as_bytes())?;
    private_file(
        &stage.path().join(".awiki-acp-profile.json"),
        b"{\"schema_version\":1}",
    )?;
    match fs::rename(stage.path(), home) {
        Ok(()) => Ok(()),
        Err(_) if initialized(home)? => Ok(()), // a concurrent initializer won
        Err(e) => Err(e).context("hermes_profile_initialization_failed"),
    }
}

// Credentials and behavioral configuration are read from the one-time seed,
// not silently changed by a later Daemon restart with another host environment.
// `env -u` names contain no values; unlike `env KEY=value`, secrets never enter argv.
pub(super) fn isolated_launch(
    config: agent_client_protocol::AcpAgentConfig,
) -> Result<agent_client_protocol::AcpAgentConfig> {
    ensure!(
        config.environment().contains_key("HERMES_HOME"),
        "hermes_profile_required"
    );
    let mut launch = agent_client_protocol::AcpAgentConfig::new("/usr/bin/env");
    for (key, _) in std::env::vars_os() {
        let Some(key) = key.to_str() else {
            bail!("hermes_environment_invalid");
        };
        if !passthrough(key) && !config.environment().contains_key(key) {
            launch = launch.args(["-u", key]);
        }
    }
    Ok(launch
        .arg(config.command().to_string_lossy())
        .args(config.arguments().iter().cloned())
        .envs(config.environment().clone()))
}

fn passthrough(key: &str) -> bool {
    matches!(
        key,
        "PATH"
            | "HOME"
            | "USER"
            | "LOGNAME"
            | "SHELL"
            | "LANG"
            | "TMPDIR"
            | "TMP"
            | "TEMP"
            | "SSL_CERT_FILE"
            | "SSL_CERT_DIR"
            | "REQUESTS_CA_BUNDLE"
            | "CURL_CA_BUNDLE"
            | "HTTP_PROXY"
            | "HTTPS_PROXY"
            | "ALL_PROXY"
            | "NO_PROXY"
            | "http_proxy"
            | "https_proxy"
            | "all_proxy"
            | "no_proxy"
            | "AWIKI_DAEMON_RUN_ID"
            | "AWIKI_DAEMON_TASK_ID"
            | "AWIKI_DAEMON_AGENT_DID"
            | "AWIKI_DAEMON_RUNTIME_PROFILE_ID"
            | "AWIKI_RUNTIME_RPC_TOKEN"
            | "AWIKI_DAEMON_RPC_SOCKET"
            | "AWIKI_DAEMON_EXECUTABLE"
    ) || key.starts_with("LC_")
}

#[cfg(test)]
#[path = "hermes_profile_tests.rs"]
mod tests;
