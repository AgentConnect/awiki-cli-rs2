use std::collections::BTreeMap;

use crate::config::Resolved;

use super::listener_service::{
    self, ListenerPlatformStatusResult, ListenerServiceConfigPlan, ListenerServiceConfigValue,
};

pub const SERVICE_PLATFORM: &str = "windows-service";
pub const START_TYPE_AUTOMATIC: &str = "automatic";
pub const FAILURE_ACTION_RESTART: &str = "restart";
pub const FAILURE_ACTION_DELAY: &str = "1s";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsListenerServiceContract {
    pub platform: &'static str,
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub arguments: Vec<String>,
    pub working_directory: String,
    pub env_vars: BTreeMap<String, String>,
    pub start_type: String,
    pub auto_start: bool,
    pub restart_on_failure: bool,
    pub restart_delay: String,
    pub log_output: bool,
    pub log_directory: String,
    pub pid_file: String,
    pub dry_contract_only: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsServiceStatus {
    pub installed: bool,
    pub running: bool,
    pub state: String,
}

pub fn service_platform() -> &'static str {
    SERVICE_PLATFORM
}

pub fn service_contract_for(resolved: &Resolved) -> WindowsListenerServiceContract {
    let plan = service_config_plan_for(resolved);
    WindowsListenerServiceContract {
        platform: SERVICE_PLATFORM,
        name: plan.name.clone(),
        display_name: plan.display_name.clone(),
        description: plan.description.clone(),
        arguments: plan.arguments.clone(),
        working_directory: plan.working_directory.clone(),
        env_vars: plan.env_vars.clone(),
        start_type: config_string(&plan, "StartType").unwrap_or_default(),
        auto_start: config_string(&plan, "StartType")
            .map(|value| value.eq_ignore_ascii_case(START_TYPE_AUTOMATIC))
            .unwrap_or(false),
        restart_on_failure: config_string(&plan, "OnFailure")
            .map(|value| value.eq_ignore_ascii_case(FAILURE_ACTION_RESTART))
            .unwrap_or(false),
        restart_delay: config_string(&plan, "OnFailureDelayDuration").unwrap_or_default(),
        log_output: config_bool(&plan, "LogOutput").unwrap_or(false),
        log_directory: config_string(&plan, "LogDirectory").unwrap_or_default(),
        pid_file: config_string(&plan, "PIDFile").unwrap_or_default(),
        dry_contract_only: true,
    }
}

pub fn service_config_plan_for(resolved: &Resolved) -> ListenerServiceConfigPlan {
    listener_service::service_config_plan_for(resolved, true)
}

pub fn service_name_for(resolved: &Resolved) -> String {
    listener_service::service_name_for(resolved)
}

pub fn service_display_name_for(resolved: &Resolved) -> String {
    listener_service::service_display_name_for(Some(resolved))
}

pub fn service_arguments() -> Vec<String> {
    listener_service::SERVICE_ARGUMENTS
        .iter()
        .map(|argument| (*argument).to_string())
        .collect()
}

pub fn status_result_from_state(state: &str) -> ListenerPlatformStatusResult {
    let status = parse_status_state(state);
    if !status.installed {
        return ListenerPlatformStatusResult::ErrNotInstalled;
    }
    if status.running {
        ListenerPlatformStatusResult::Running
    } else {
        ListenerPlatformStatusResult::NotRunning
    }
}

pub fn parse_status_state(state: &str) -> WindowsServiceStatus {
    let normalized = normalize_state(state);
    let installed = !is_not_installed_state(&normalized);
    let running = matches!(
        normalized.as_str(),
        "running" | "start-pending" | "continue-pending"
    );
    WindowsServiceStatus {
        installed,
        running,
        state: normalized,
    }
}

fn normalize_state(state: &str) -> String {
    let state = extract_state_value(state).unwrap_or(state);
    let normalized = state
        .trim()
        .trim_matches('"')
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .fold(String::new(), |mut output, ch| {
            if ch == '-' {
                if !output.ends_with('-') {
                    output.push(ch);
                }
            } else {
                output.push(ch);
            }
            output
        })
        .trim_matches('-')
        .to_string();
    trim_state_prefixes(&normalized)
}

fn extract_state_value(input: &str) -> Option<&str> {
    for line in input.lines() {
        let (key, value) = line.split_once(':')?;
        let key = normalize_state(key);
        if matches!(key.as_str(), "state" | "status" | "current-state") {
            return Some(value.split('(').next().unwrap_or(value).trim());
        }
    }
    None
}

fn trim_state_prefixes(state: &str) -> String {
    let without_number = state.trim_start_matches(|ch: char| ch.is_ascii_digit() || ch == '-');
    let without_number = without_number
        .strip_prefix("state-")
        .unwrap_or(without_number);
    let without_number = without_number
        .strip_prefix("status-")
        .unwrap_or(without_number);
    without_number
        .strip_prefix("service-")
        .unwrap_or(without_number)
        .to_string()
}

fn is_not_installed_state(state: &str) -> bool {
    matches!(
        state,
        "" | "not-found" | "notinstalled" | "not-installed" | "missing"
    ) || state.contains("does-not-exist")
        || state.contains("not-installed")
}

fn config_string(plan: &ListenerServiceConfigPlan, key: &str) -> Option<String> {
    match plan.options.get(key) {
        Some(ListenerServiceConfigValue::String(value)) => Some(value.clone()),
        Some(ListenerServiceConfigValue::Bool(_)) | None => None,
    }
}

fn config_bool(plan: &ListenerServiceConfigPlan, key: &str) -> Option<bool> {
    match plan.options.get(key) {
        Some(ListenerServiceConfigValue::Bool(value)) => Some(*value),
        Some(ListenerServiceConfigValue::String(_)) | None => None,
    }
}
