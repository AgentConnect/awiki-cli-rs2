//! Native working-directory and process-tree boundary for SDK-owned ACP children.
//! The unnamed job is installed on this helper before any client is spawned.
//! Its non-inheritable last handle is closed by the kernel when this helper exits.
use agent_client_protocol::AcpAgentConfig;
use anyhow::{bail, Context, Result};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use windows_sys::Win32::{
    Foundation::*,
    System::{JobObjects::*, Threading::*},
};

pub(super) fn wrap(config: AcpAgentConfig, cwd: &Path) -> Result<AcpAgentConfig> {
    let executable = std::env::current_exe().context("acp_launcher_unavailable")?;
    Ok(AcpAgentConfig::new(executable)
        .arg("__acp-windows-child")
        .arg(std::process::id().to_string())
        .arg(cwd.to_string_lossy())
        .arg(config.command().to_string_lossy())
        .args(config.arguments().iter().cloned())
        .envs(config.environment().clone()))
}

/// Retain only explicitly selected environment names; values stay in the child
/// environment. The inherited cwd lets an outer workspace launcher remain owner.
pub(super) fn isolate(
    config: AcpAgentConfig,
    passthrough: impl Fn(&str) -> bool,
) -> Result<AcpAgentConfig> {
    let mut keys = BTreeSet::new();
    for (key, _) in std::env::vars_os() {
        let key = key.to_str().context("hermes_environment_invalid")?;
        if passthrough(key) {
            keys.insert(key.to_owned());
        }
    }
    keys.extend(config.environment().keys().cloned());
    let executable = std::env::current_exe().context("acp_launcher_unavailable")?;
    Ok(AcpAgentConfig::new(executable)
        .arg("__acp-windows-child")
        .arg(std::process::id().to_string())
        .arg("--inherit-cwd")
        .arg("--isolated-env")
        .arg(serde_json::to_string(&keys)?)
        .arg("--")
        .arg(config.command().to_string_lossy())
        .args(config.arguments().iter().cloned())
        .envs(config.environment().clone()))
}

/// Private executable entrypoint. Never print argv, environment, or ACP bytes.
/// ExitProcess closes the job even after an abrupt SDK kill; no drop handler is
/// used after assigning this helper itself, since closing that job kills us too.
pub fn run(arguments: impl IntoIterator<Item = OsString>) -> ! {
    let code = match execute(arguments.into_iter().collect()) {
        Ok(code) => code,
        Err(_) => {
            eprintln!("acp_windows_child_failed");
            2
        }
    };
    std::process::exit(code)
}

fn execute(arguments: Vec<OsString>) -> Result<i32> {
    if arguments.len() < 3 {
        bail!("invalid_launcher_arguments");
    }
    let parent: u32 = arguments[0].to_str().context("invalid_parent")?.parse()?;
    let cwd = if arguments[1] == "--inherit-cwd" {
        std::env::current_dir().context("cwd_unavailable")?
    } else {
        PathBuf::from(&arguments[1])
    };
    let isolated_keys = if arguments[2] == "--isolated-env" {
        if arguments.len() < 6 || arguments[4] != "--" {
            bail!("invalid_environment_arguments");
        }
        let keys: Vec<String> =
            serde_json::from_str(arguments[3].to_str().context("invalid_environment_names")?)?;
        if keys.iter().any(|k| k.is_empty() || k.contains(['=', '\0'])) {
            bail!("invalid_environment_names");
        }
        Some(
            keys.into_iter()
                .map(|k| k.to_uppercase())
                .collect::<BTreeSet<_>>(),
        )
    } else {
        None
    };
    let command_index = if isolated_keys.is_some() { 5 } else { 2 };
    if parent == 0 || parent == std::process::id() || !cwd.is_absolute() || !cwd.is_dir() {
        bail!("invalid_launcher_context");
    }
    // cmd.exe launchers reject a verbatim local-drive cwd as if it were UNC.
    // The signed/session cwd stays canonical; use its equivalent DOS spelling
    // only for the Win32 child working-directory field.
    let cwd = match cwd.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(local) if local.as_bytes().get(1) == Some(&b':') => PathBuf::from(local),
        _ => cwd,
    };
    // Hold a kernel handle to the original parent; PID reuse after this point
    // cannot keep a client alive after its actual Daemon has terminated.
    let parent_handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, parent) };
    if parent_handle == 0 {
        bail!("parent_unavailable");
    }
    if unsafe { WaitForSingleObject(parent_handle, 0) } != WAIT_TIMEOUT {
        unsafe {
            CloseHandle(parent_handle);
        }
        bail!("parent_unavailable");
    }
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job == 0 {
        unsafe {
            CloseHandle(parent_handle);
        }
        bail!("job_unavailable");
    }
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let configured = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const _,
            std::mem::size_of_val(&limits) as u32,
        )
    };
    if configured == 0 || unsafe { AssignProcessToJobObject(job, GetCurrentProcess()) } == 0 {
        unsafe {
            CloseHandle(job);
            CloseHandle(parent_handle);
        }
        bail!("job_unavailable");
    }
    // job is intentionally held until this process exits. It must not be
    // inherited by the client, and no breakaway flags are granted.
    std::thread::spawn(move || {
        unsafe {
            WaitForSingleObject(parent_handle, INFINITE);
            CloseHandle(parent_handle);
        }
        std::process::exit(1);
    });
    let mut command = Command::new(&arguments[command_index]);
    if let Some(keys) = isolated_keys {
        let environment = std::env::vars_os().filter(|(key, _)| {
            key.to_str()
                .is_some_and(|key| keys.contains(&key.to_uppercase()))
        });
        command.env_clear().envs(environment);
    }
    let status = command
        .args(&arguments[command_index + 1..])
        .current_dir(&cwd)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .context("client_unavailable")?;
    Ok(status.code().unwrap_or(1))
}
