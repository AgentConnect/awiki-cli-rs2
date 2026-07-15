use crate::workspace_config::Resolved;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::listener::{self, Status};
use super::listener_service::{self, ListenerServiceConfigValue};

pub const SERVICE_PLATFORM: &str = "launchd";

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LaunchdAgent {
    pub label: String,
    pub path: PathBuf,
    pub content: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LaunchdStatus {
    pub installed: bool,
    pub loaded: bool,
    pub running: bool,
    pub pid: Option<u32>,
    pub last_exit_status: Option<i32>,
    pub raw_state: String,
}

pub fn service_platform() -> &'static str {
    SERVICE_PLATFORM
}

pub fn label_for(resolved: &Resolved) -> String {
    listener_service::service_name_for(resolved)
}

pub fn name_for(resolved: &Resolved) -> String {
    label_for(resolved)
}

pub fn plist_path_for(resolved: &Resolved) -> anyhow::Result<PathBuf> {
    plist_path_for_home(resolved, user_home()?)
}

pub fn plist_path_for_home(resolved: &Resolved, home: impl AsRef<Path>) -> anyhow::Result<PathBuf> {
    Ok(home
        .as_ref()
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{}.plist", label_for(resolved))))
}

pub fn agent_for(resolved: &Resolved) -> anyhow::Result<LaunchdAgent> {
    agent_for_home_and_executable(resolved, user_home()?, current_executable()?)
}

pub fn agent_for_executable(
    resolved: &Resolved,
    executable: impl AsRef<Path>,
) -> anyhow::Result<LaunchdAgent> {
    agent_for_home_and_executable(resolved, user_home()?, executable)
}

pub fn agent_for_home_and_executable(
    resolved: &Resolved,
    home: impl AsRef<Path>,
    executable: impl AsRef<Path>,
) -> anyhow::Result<LaunchdAgent> {
    let label = label_for(resolved);
    Ok(LaunchdAgent {
        label,
        path: plist_path_for_home(resolved, home)?,
        content: plist_content_for_executable(resolved, executable),
    })
}

pub fn plist_content_for(resolved: &Resolved) -> anyhow::Result<String> {
    Ok(plist_content_for_executable(
        resolved,
        current_executable()?,
    ))
}

pub fn plist_content_for_executable(resolved: &Resolved, executable: impl AsRef<Path>) -> String {
    let plan = listener_service::service_config_plan_for(resolved, false);
    let executable = executable.as_ref().to_string_lossy().into_owned();
    let log_dir = plan
        .options
        .get("LogDirectory")
        .and_then(config_string)
        .unwrap_or_else(|| resolved.paths.logs_dir.clone());
    let stdout_log = Path::new(&log_dir).join(format!("{}.out.log", plan.name));
    let stderr_log = Path::new(&log_dir).join(format!("{}.err.log", plan.name));
    let run_at_load = config_bool(plan.options.get("RunAtLoad")).unwrap_or(true);
    let keep_alive = config_bool(plan.options.get("KeepAlive")).unwrap_or(true);
    let session_create = config_bool(plan.options.get("SessionCreate")).unwrap_or(false);

    let mut content = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n\
<dict>\n\
\t<key>Disabled</key>\n\
\t<false/>\n",
    );

    if !plan.env_vars.is_empty() {
        content.push_str("\t<key>EnvironmentVariables</key>\n\t<dict>\n");
        for (key, value) in &plan.env_vars {
            content.push_str(&format!(
                "\t\t<key>{}</key>\n\t\t<string>{}</string>\n",
                xml_escape(key),
                xml_escape(value)
            ));
        }
        content.push_str("\t</dict>\n");
    }

    content.push_str(&format!(
        "\t<key>KeepAlive</key>\n\
\t<{keep_alive}/>\n\
\t<key>Label</key>\n\
\t<string>{label}</string>\n\
\t<key>ProgramArguments</key>\n\
\t<array>\n\
\t\t<string>{executable}</string>\n",
        keep_alive = plist_bool(keep_alive),
        label = xml_escape(&plan.name),
        executable = xml_escape(&executable),
    ));

    for argument in &plan.arguments {
        content.push_str(&format!("\t\t<string>{}</string>\n", xml_escape(argument)));
    }

    content.push_str(&format!(
        "\t</array>\n\
\t<key>RunAtLoad</key>\n\
\t<{run_at_load}/>\n\
\t<key>SessionCreate</key>\n\
\t<{session_create}/>\n\
\t<key>StandardErrorPath</key>\n\
\t<string>{stderr_log}</string>\n\
\t<key>StandardOutPath</key>\n\
\t<string>{stdout_log}</string>\n\
\t<key>WorkingDirectory</key>\n\
\t<string>{working_directory}</string>\n\
</dict>\n\
</plist>\n",
        run_at_load = plist_bool(run_at_load),
        session_create = plist_bool(session_create),
        stderr_log = xml_escape(&stderr_log.to_string_lossy()),
        stdout_log = xml_escape(&stdout_log.to_string_lossy()),
        working_directory = xml_escape(&plan.working_directory),
    ));
    content
}

pub fn status(resolved: &Resolved) -> anyhow::Result<LaunchdStatus> {
    require_supported()?;
    status_with_runner(resolved, run_launchctl)
}

pub fn install(resolved: &Resolved) -> anyhow::Result<Status> {
    require_supported()?;
    write_agent(resolved)?;
    listener_status(resolved)
}

pub fn start(resolved: &Resolved) -> anyhow::Result<Status> {
    require_supported()?;
    if super::resolve(resolved).mode != super::bridge::MODE_WEBSOCKET {
        anyhow::bail!("runtime mode must be websocket before starting the listener");
    }
    write_agent(resolved)?;
    let current = status(resolved)?;
    if current.running
        && listener::bridge_endpoint_available(&listener::paths(resolved)?.socket_path)
    {
        return listener_status(resolved);
    }

    let expected_boot_id = listener_service::prepare_expected_boot_id(resolved)?;
    if current.loaded {
        kickstart(resolved)?;
    } else {
        bootstrap(resolved)?;
    }
    wait_for_listener_status(resolved, true, &expected_boot_id)
}

pub fn stop(resolved: &Resolved) -> anyhow::Result<Status> {
    require_supported()?;
    let current = status(resolved)?;
    if current.loaded {
        let agent = agent_for(resolved)?;
        let domain = launch_domain();
        run_launchctl(&[
            "bootout",
            domain.as_str(),
            agent.path.to_string_lossy().as_ref(),
        ])?;
    }
    listener_service::cleanup_runtime_artifacts(resolved);
    wait_for_listener_status(resolved, false, "")
}

pub fn restart(resolved: &Resolved) -> anyhow::Result<Status> {
    require_supported()?;
    if super::resolve(resolved).mode != super::bridge::MODE_WEBSOCKET {
        anyhow::bail!("runtime mode must be websocket before starting the listener");
    }
    if !status(resolved)?.installed {
        anyhow::bail!("listener service is not installed");
    }
    write_agent(resolved)?;
    let expected_boot_id = listener_service::prepare_expected_boot_id(resolved)?;
    if status(resolved)?.loaded {
        let agent = agent_for(resolved)?;
        let domain = launch_domain();
        run_launchctl(&[
            "bootout",
            domain.as_str(),
            agent.path.to_string_lossy().as_ref(),
        ])?;
    }
    bootstrap(resolved)?;
    wait_for_listener_status(resolved, true, &expected_boot_id)
}

pub fn uninstall(resolved: &Resolved) -> anyhow::Result<Status> {
    require_supported()?;
    let current = status(resolved)?;
    let agent = agent_for(resolved)?;
    if current.loaded {
        let domain = launch_domain();
        run_launchctl(&[
            "bootout",
            domain.as_str(),
            agent.path.to_string_lossy().as_ref(),
        ])?;
    }
    if agent.path.exists() {
        fs::remove_file(&agent.path)
            .map_err(|err| anyhow::anyhow!("remove listener LaunchAgent: {err}"))?;
    }
    listener_service::cleanup_runtime_artifacts(resolved);
    listener_status(resolved)
}

pub fn listener_status(resolved: &Resolved) -> anyhow::Result<Status> {
    let service = status(resolved)?;
    let mut result = listener::status_for(
        resolved,
        service.installed,
        service.running,
        service_platform(),
    )?;
    if service.installed && !service.loaded {
        result
            .warnings
            .push("listener LaunchAgent is installed but not loaded".to_string());
    }
    if service.last_exit_status.is_some_and(|code| code != 0) {
        result.warnings.push(format!(
            "listener LaunchAgent last exited with status {}",
            service.last_exit_status.unwrap_or_default()
        ));
    }
    Ok(result)
}

pub fn status_with_runner(
    resolved: &Resolved,
    mut runner: impl FnMut(&[&str]) -> anyhow::Result<String>,
) -> anyhow::Result<LaunchdStatus> {
    let agent = agent_for(resolved)?;
    if !agent.path.is_file() {
        return Ok(LaunchdStatus {
            installed: false,
            loaded: false,
            running: false,
            pid: None,
            last_exit_status: None,
            raw_state: String::new(),
        });
    }
    let target = launch_target(resolved);
    match runner(&["print", target.as_str()]) {
        Ok(output) => {
            let mut status = parse_launchctl_status(&output);
            status.installed = true;
            status.loaded = true;
            Ok(status)
        }
        Err(err) => Ok(LaunchdStatus {
            installed: true,
            loaded: false,
            running: false,
            pid: None,
            last_exit_status: None,
            raw_state: err.to_string(),
        }),
    }
}

pub fn parse_launchctl_status(output: &str) -> LaunchdStatus {
    let pid = parse_u32_after_keys(output, &["pid", "PID"]).filter(|pid| *pid > 0);
    let last_exit_status = parse_i32_after_keys(
        output,
        &["last exit code", "last exit status", "LastExitStatus"],
    );
    let raw_state = first_meaningful_line(output)
        .unwrap_or_default()
        .to_string();
    let not_installed = looks_not_installed(output);
    let installed = pid.is_some()
        || last_exit_status.is_some()
        || (!output.trim().is_empty() && !not_installed);
    let normalized = output.to_ascii_lowercase();
    let running = pid.is_some()
        || normalized.lines().any(|line| {
            let line = line.trim();
            line == "state = running" || line == "state: running"
        });

    LaunchdStatus {
        installed,
        loaded: installed,
        running,
        pid,
        last_exit_status,
        raw_state,
    }
}

fn write_agent(resolved: &Resolved) -> anyhow::Result<()> {
    let agent = agent_for(resolved)?;
    if let Some(parent) = agent.path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| anyhow::anyhow!("create LaunchAgents directory: {err}"))?;
    }
    fs::create_dir_all(&resolved.paths.logs_dir)
        .map_err(|err| anyhow::anyhow!("create listener log directory: {err}"))?;
    let should_write = fs::read_to_string(&agent.path)
        .map(|current| current != agent.content)
        .unwrap_or(true);
    if should_write {
        fs::write(&agent.path, agent.content.as_bytes())
            .map_err(|err| anyhow::anyhow!("write listener LaunchAgent: {err}"))?;
    }
    Ok(())
}

fn bootstrap(resolved: &Resolved) -> anyhow::Result<()> {
    let agent = agent_for(resolved)?;
    let domain = launch_domain();
    run_launchctl(&[
        "bootstrap",
        domain.as_str(),
        agent.path.to_string_lossy().as_ref(),
    ])?;
    kickstart(resolved)
}

fn kickstart(resolved: &Resolved) -> anyhow::Result<()> {
    let target = launch_target(resolved);
    run_launchctl(&["kickstart", "-k", target.as_str()]).map(|_| ())
}

fn wait_for_listener_status(
    resolved: &Resolved,
    want_running: bool,
    expected_boot_id: &str,
) -> anyhow::Result<Status> {
    let wait_for_bridge = want_running
        && super::resolve(resolved).mode == super::bridge::MODE_WEBSOCKET
        && super::resolve(resolved).listener.enabled;
    listener_service::wait_for_service_status_with(
        || listener_status(resolved),
        want_running,
        wait_for_bridge,
        expected_boot_id,
        Duration::from_secs(15),
        Duration::from_millis(250),
    )
}

fn launch_domain() -> String {
    format!("gui/{}", current_uid())
}

fn launch_target(resolved: &Resolved) -> String {
    format!("{}/{}", launch_domain(), label_for(resolved))
}

#[cfg(target_os = "macos")]
fn current_uid() -> u32 {
    unsafe { getuid() }
}

#[cfg(not(target_os = "macos"))]
fn current_uid() -> u32 {
    0
}

#[cfg(target_os = "macos")]
extern "C" {
    fn getuid() -> u32;
}

fn require_supported() -> anyhow::Result<()> {
    if cfg!(target_os = "macos") {
        return Ok(());
    }
    anyhow::bail!("launchd listener services require macOS")
}

fn run_launchctl(args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("launchctl")
        .args(args)
        .output()
        .map_err(|err| anyhow::anyhow!("run launchctl {}: {err}", args.join(" ")))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if stderr.is_empty() { stdout } else { stderr };
    anyhow::bail!(
        "launchctl {} failed: {}",
        args.join(" "),
        if detail.is_empty() {
            output.status.to_string()
        } else {
            detail
        }
    )
}

fn user_home() -> anyhow::Result<PathBuf> {
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("HOME is required for launchd listener service contract"))
}

fn current_executable() -> anyhow::Result<PathBuf> {
    env::current_exe().map_err(|err| {
        anyhow::anyhow!("resolve current executable for launchd listener service: {err}")
    })
}

fn config_string(value: &ListenerServiceConfigValue) -> Option<String> {
    match value {
        ListenerServiceConfigValue::String(value) => Some(value.clone()),
        ListenerServiceConfigValue::Bool(_) => None,
    }
}

fn config_bool(value: Option<&ListenerServiceConfigValue>) -> Option<bool> {
    match value {
        Some(ListenerServiceConfigValue::Bool(value)) => Some(*value),
        Some(ListenerServiceConfigValue::String(_)) | None => None,
    }
}

fn plist_bool(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn xml_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&#34;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn first_meaningful_line(output: &str) -> Option<&str> {
    output.lines().map(str::trim).find(|line| !line.is_empty())
}

fn looks_not_installed(output: &str) -> bool {
    let normalized = output.to_ascii_lowercase();
    normalized.contains("not-found")
        || normalized.contains("could not find")
        || normalized.contains("unknown service")
        || normalized.contains("unrecognized service")
}

fn parse_u32_after_keys(output: &str, keys: &[&str]) -> Option<u32> {
    keys.iter()
        .find_map(|key| parse_i64_after_key(output, key))
        .and_then(|value| u32::try_from(value).ok())
}

fn parse_i32_after_keys(output: &str, keys: &[&str]) -> Option<i32> {
    keys.iter()
        .find_map(|key| parse_i64_after_key(output, key))
        .and_then(|value| i32::try_from(value).ok())
}

fn parse_i64_after_key(output: &str, key: &str) -> Option<i64> {
    for line in output.lines() {
        let normalized = line.to_ascii_lowercase();
        let key = key.to_ascii_lowercase();
        let Some(index) = normalized.find(&key) else {
            continue;
        };
        let after_key = &line[index + key.len()..];
        if let Some(value) = first_integer(after_key) {
            return Some(value);
        }
    }
    None
}

fn first_integer(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let sign = bytes[index] == b'-';
        let digit = bytes[index].is_ascii_digit();
        if sign || digit {
            let start = index;
            if sign {
                index += 1;
            }
            let digits_start = index;
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
            if index > digits_start {
                return value[start..index].parse::<i64>().ok();
            }
        } else {
            index += 1;
        }
    }
    None
}
