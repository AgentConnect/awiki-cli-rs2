use crate::workspace_config::Resolved;
use std::env;
use std::path::{Path, PathBuf};

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

pub fn parse_launchctl_status(output: &str) -> LaunchdStatus {
    let pid = parse_u32_after_key(output, "PID").filter(|pid| *pid > 0);
    let last_exit_status = parse_i32_after_key(output, "LastExitStatus");
    let raw_state = first_meaningful_line(output)
        .unwrap_or_default()
        .to_string();
    let not_installed = looks_not_installed(output);
    let installed = pid.is_some()
        || last_exit_status.is_some()
        || (!output.trim().is_empty() && !not_installed);

    LaunchdStatus {
        installed,
        running: pid.is_some(),
        pid,
        last_exit_status,
        raw_state,
    }
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

fn parse_u32_after_key(output: &str, key: &str) -> Option<u32> {
    parse_i64_after_key(output, key).and_then(|value| u32::try_from(value).ok())
}

fn parse_i32_after_key(output: &str, key: &str) -> Option<i32> {
    parse_i64_after_key(output, key).and_then(|value| i32::try_from(value).ok())
}

fn parse_i64_after_key(output: &str, key: &str) -> Option<i64> {
    for line in output.lines() {
        let Some(index) = line.find(key) else {
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
