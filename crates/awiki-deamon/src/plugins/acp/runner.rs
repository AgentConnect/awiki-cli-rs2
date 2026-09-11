use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde_json::Value;

use super::connection::{AcpProcessPool, AcpProcessSpec, AcpSessionRef};
use super::protocol::AcpPermissionPolicy;
use super::ACP_RUNTIME_PLUGIN_ID;
use crate::runtime::prompt_context::{render_invocation_context_prompt, RuntimePromptContext};
use crate::runtime::{
    RuntimeInstallStatus, RuntimeLaunchContext, RuntimeLaunchOutcome, RuntimePlugin,
    RuntimeRunStatus, RuntimeTask,
};
use crate::security::runtime_token::current_time_millis;
use crate::state::{AcpNativeSessionRecord, AcpRuntimeProfileRecord, DaemonState};

#[derive(Debug, Clone)]
pub struct AcpRuntimePlugin {
    pool: AcpProcessPool,
    profile: AcpRuntimeProfileRecord,
    state: DaemonState,
}

impl AcpRuntimePlugin {
    pub fn with_state(
        pool: AcpProcessPool,
        profile: AcpRuntimeProfileRecord,
        state: DaemonState,
    ) -> Self {
        Self {
            pool,
            profile,
            state,
        }
    }

    fn process_spec(&self) -> Result<AcpProcessSpec> {
        self.process_spec_with_env(BTreeMap::new())
    }

    fn process_spec_with_env(&self, overrides: BTreeMap<String, String>) -> Result<AcpProcessSpec> {
        let catalog = super::catalog::entry(&self.profile.acp_agent_id)?;
        let program = self
            .profile
            .entry_command_json
            .get("program")
            .and_then(Value::as_str)
            .context("ACP entry command is missing program")?;
        let args = self
            .profile
            .entry_command_json
            .get("args")
            .and_then(Value::as_array)
            .context("ACP entry command is missing args")?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .context("ACP entry command args must contain only strings")
            })
            .collect::<Result<Vec<_>>>()?;
        let cwd = self
            .profile
            .entry_command_json
            .get("cwd")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .unwrap_or_else(|| self.profile.install_root.clone());
        let permission_policy = match self.profile.permission_policy.as_str() {
            "allow-once" => AcpPermissionPolicy::AllowOnce,
            "reject-once" => AcpPermissionPolicy::RejectOnce,
            other => bail!("unsupported ACP permission policy: {other}"),
        };
        let mut env = load_profile_environment(&self.profile)?;
        add_safe_process_environment(&mut env, catalog.inherited_process_env_names);
        for (name, value) in overrides {
            if self.profile.credential_env_names.contains(&name) {
                env.insert(name, value);
            }
        }
        for required in catalog.required_credential_env_names {
            if env.get(*required).is_none_or(|value| value.is_empty()) {
                bail!("ACP required credential {required} is unavailable");
            }
        }
        Ok(AcpProcessSpec {
            runner_id: format!(
                "{}:{}",
                self.profile.runtime_profile_id,
                self.profile.install_root.display()
            ),
            program: PathBuf::from(program),
            args,
            cwd,
            env,
            permission_policy,
        })
    }

    pub(super) fn smoke_check_with_env(&self, env: BTreeMap<String, String>) -> Result<()> {
        let spec = self.process_spec_with_env(env)?;
        let runner = self.pool.ensure_process(&spec)?;
        self.pool.terminate(&runner)
    }

    fn validate_binding(&self, context: &RuntimeLaunchContext) -> Result<()> {
        if context.run.runtime_plugin_id != ACP_RUNTIME_PLUGIN_ID
            || context.run.runtime_profile_id != self.profile.runtime_profile_id
            || context.run.agent_did != self.profile.agent_did
            || context.task.agent_did != self.profile.agent_did
            || context.run.task_id != context.task.task_id
        {
            bail!("ACP runtime profile binding does not match launch context");
        }
        context.task.validate()?;
        Ok(())
    }
}

impl RuntimePlugin for AcpRuntimePlugin {
    fn plugin_id(&self) -> &str {
        ACP_RUNTIME_PLUGIN_ID
    }

    fn check_install_status(&self) -> Result<RuntimeInstallStatus> {
        self.profile.validate()?;
        let spec = match self.process_spec() {
            Ok(spec) => spec,
            Err(error) => {
                let detail = if error.to_string().contains("required credential") {
                    "ACP required credential is unavailable".to_string()
                } else {
                    super::connection::redact_line(&error.to_string())
                };
                return Ok(RuntimeInstallStatus {
                    installed: false,
                    detail: Some(detail),
                });
            }
        };
        let installed = self.profile.status == "ready"
            && self.profile.config_path.is_file()
            && self.profile.cwd_root.is_dir()
            && self.profile.install_root.is_dir()
            && program_is_available(&spec.program);
        Ok(RuntimeInstallStatus {
            installed,
            detail: Some(format!(
                "ACP agent {} ({})",
                self.profile.acp_agent_id, self.profile.install_mode
            )),
        })
    }

    fn launch_run(&self, context: RuntimeLaunchContext) -> Result<RuntimeLaunchOutcome> {
        self.validate_binding(&context)?;
        let install_status = self.check_install_status()?;
        if !install_status.installed {
            bail!(
                "ACP runtime profile is not installed or ready: {}",
                install_status
                    .detail
                    .as_deref()
                    .unwrap_or("setup is required")
            );
        }

        let previous_epoch = self
            .state
            .latest_acp_connection_epoch(&self.profile.runtime_profile_id)?;
        let runner = self
            .pool
            .ensure_process_after(&self.process_spec()?, previous_epoch)
            .context("ensure ACP process")?;
        self.state.mark_acp_sessions_stale_before_epoch(
            &self.profile.runtime_profile_id,
            runner.connection_epoch,
        )?;

        let route_key = acp_session_route_key(&self.profile, &context.task)?;
        let previous_route_epoch = self.state.latest_acp_session_epoch_by_route(&route_key)?;
        let stored_session = self
            .state
            .load_active_acp_session_by_route(&route_key, runner.connection_epoch)?;
        let session_recreated = stored_session.is_none()
            && previous_route_epoch.is_some_and(|epoch| epoch < runner.connection_epoch);
        let session = match stored_session {
            Some(record) => AcpSessionRef {
                runner_id: runner.runner_id.clone(),
                connection_epoch: runner.connection_epoch,
                session_id: record.acp_session_id,
            },
            None => {
                let session = self
                    .pool
                    .create_session(&runner, &self.profile.cwd_root)
                    .context("create ACP session")?;
                let now = current_time_millis()?;
                self.state
                    .store_acp_native_session(&AcpNativeSessionRecord {
                        route_key: route_key.clone(),
                        agent_did: self.profile.agent_did.clone(),
                        runtime_profile_id: self.profile.runtime_profile_id.clone(),
                        acp_session_id: session.session_id.clone(),
                        connection_epoch: runner.connection_epoch,
                        status: "active".to_string(),
                        created_at_ms: now,
                        updated_at_ms: now,
                    })?;
                session
            }
        };

        let prompt_context = RuntimePromptContext::from_task(&context.task);
        let prompt = format!(
            "{}\n\n[User Message]\n{}",
            render_invocation_context_prompt(&prompt_context, &context.preferred_language),
            context.task.text
        );
        let outcome = self
            .pool
            .submit_prompt(&runner, &session, &prompt)
            .context("submit ACP prompt")?;
        let session_status =
            session_recreated.then_some("ACP subprocess restarted; created a new session");

        Ok(RuntimeLaunchOutcome {
            run_id: context.run.run_id,
            status: RuntimeRunStatus::Running,
            exit_code: None,
            callbacks: Vec::new(),
            metadata: serde_json::json!({
                "final_text": outcome.final_text,
                "error": null,
                "stop_reason": outcome.stop_reason,
                "permission_requests": outcome.permission_requests,
                "connection_epoch": runner.connection_epoch,
                "session_recreated": session_recreated,
                "session_status": session_status,
                "acp_session_id": session.session_id,
            }),
        })
    }
}

pub fn acp_session_route_key(
    profile: &AcpRuntimeProfileRecord,
    task: &RuntimeTask,
) -> Result<String> {
    profile.validate()?;
    task.validate()?;
    if task.agent_did != profile.agent_did {
        bail!("ACP runtime profile binding does not match task");
    }
    Ok(format!(
        "acp:{}:{}:{}:{}:conversation",
        profile.agent_did,
        task.controller_scope_key,
        task.conversation_scope.kind_str(),
        task.conversation_scope.scope_key(),
    ))
}

fn program_is_available(program: &Path) -> bool {
    if program.components().count() > 1 || program.is_absolute() {
        return program.is_file();
    }
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).any(|directory| directory.join(program).is_file()))
        .unwrap_or(false)
}

fn load_profile_environment(profile: &AcpRuntimeProfileRecord) -> Result<BTreeMap<String, String>> {
    let mut env = BTreeMap::new();
    let dotenv_path = profile
        .config_path
        .parent()
        .context("ACP config_path must have a parent")?
        .join(".env");
    let dotenv_content = match std::fs::symlink_metadata(&dotenv_path) {
        Ok(_) => Some(read_profile_dotenv(&dotenv_path)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error)
                .with_context(|| format!("inspect ACP credential file {}", dotenv_path.display()))
        }
    };
    if let Some(content) = dotenv_content {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (name, encoded) = line
                .split_once('=')
                .context("invalid ACP credential file entry")?;
            if !profile
                .credential_env_names
                .iter()
                .any(|allowed| allowed == name)
            {
                bail!("ACP credential file contains an unlisted variable name");
            }
            let value: String =
                serde_json::from_str(encoded).context("parse ACP credential file value")?;
            env.insert(name.to_string(), value);
        }
    }
    for name in &profile.credential_env_names {
        if !env.contains_key(name) {
            if let Ok(value) = std::env::var(name) {
                env.insert(name.clone(), value);
            }
        }
    }
    Ok(env)
}

fn read_profile_dotenv(path: &Path) -> Result<String> {
    use std::io::Read;

    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("inspect ACP credential file {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("ACP credential file must be a regular file, not a symlink");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .with_context(|| format!("open ACP credential file {}", path.display()))?;
        let opened_metadata = file
            .metadata()
            .with_context(|| format!("inspect opened ACP credential file {}", path.display()))?;
        if !opened_metadata.is_file() {
            bail!("ACP credential file must be a regular file, not a symlink");
        }
        if opened_metadata.permissions().mode() & 0o077 != 0 {
            bail!("ACP credential file permissions must be 0600 or stricter");
        }
        let mut content = String::new();
        file.read_to_string(&mut content)
            .with_context(|| format!("read ACP credential file {}", path.display()))?;
        return Ok(content);
    }
    #[cfg(not(unix))]
    std::fs::read_to_string(path)
        .with_context(|| format!("read ACP credential file {}", path.display()))
}

fn add_safe_process_environment(env: &mut BTreeMap<String, String>, inherited_names: &[&str]) {
    if let Some(path) =
        crate::cli_runtime_env::cli_child_path().or_else(|| std::env::var_os("PATH"))
    {
        if let Some(path) = path.to_str() {
            env.insert("PATH".to_string(), path.to_string());
        }
    }
    for name in inherited_names {
        if let Ok(value) = std::env::var(name) {
            env.insert(name.to_string(), value);
        }
    }
}
