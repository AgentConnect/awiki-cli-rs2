use super::upgrader::{Context, MigrationError};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const LEGACY_SKILL_INSTALL_DIR_NAME: &str = "awiki-agent-id-message";
const LEGACY_HEARTBEAT_SECTION_START: &str = "<!-- awiki-heartbeat-start -->";
const LEGACY_HEARTBEAT_SECTION_END: &str = "<!-- awiki-heartbeat-end -->";
const LEGACY_MACOS_LISTENER_LABEL: &str = "com.awiki.ws-listener";
const LEGACY_LINUX_LISTENER_UNIT: &str = "awiki-ws-listener.service";
const LEGACY_WINDOWS_LISTENER_TASK_NAME: &str = "awiki-ws-listener";

pub fn apply_workspace_v1_to_v2_cleanup(context: &mut Context) -> Result<(), MigrationError> {
    apply_workspace_v1_to_v2_cleanup_optional(Some(context))
}

pub fn apply_workspace_v1_to_v2_cleanup_optional(
    context: Option<&mut Context>,
) -> Result<(), MigrationError> {
    let context = context.ok_or_else(|| {
        MigrationError::Message("workspace upgrade context is required".to_string())
    })?;
    context.warnings.extend(cleanup_legacy_skill_artifacts());
    Ok(())
}

fn cleanup_legacy_skill_artifacts() -> Vec<String> {
    let home_dir = match legacy_cleanup_user_home() {
        Ok(home_dir) => home_dir,
        Err(err) => {
            return vec![format!(
                "Legacy awiki skill cleanup skipped: resolve home directory failed: {err}"
            )];
        }
    };
    let env = LegacyCleanupEnv::from_process();
    let mut runner = system_command_runner;
    cleanup_legacy_skill_artifacts_for_home(
        &home_dir,
        LegacyCleanupPlatform::current(),
        &env,
        &mut runner,
    )
}

fn cleanup_legacy_skill_artifacts_for_home(
    home_dir: &Path,
    platform: LegacyCleanupPlatform,
    env: &LegacyCleanupEnv,
    runner: &mut dyn FnMut(&str, &[String]) -> Result<(), String>,
) -> Vec<String> {
    let mut warnings = uninstall_legacy_listener_service(home_dir, platform, env, runner);

    for path in legacy_skill_install_dirs(home_dir) {
        if !path.exists() {
            continue;
        }
        if let Err(err) = remove_all(&path) {
            warnings.push(format!(
                "Failed to remove legacy awiki skill path {}: {err}",
                display_path(&path)
            ));
        }
    }

    if let Err(err) = remove_legacy_heartbeat_section(home_dir, env) {
        warnings.push(format!(
            "Failed to remove legacy awiki heartbeat section: {err}"
        ));
    }
    warnings
}

fn legacy_skill_install_dirs(home_dir: &Path) -> Vec<PathBuf> {
    vec![
        home_dir
            .join(".openclaw")
            .join("skills")
            .join(LEGACY_SKILL_INSTALL_DIR_NAME),
        home_dir
            .join(".openclaw")
            .join("workspace")
            .join("skills")
            .join(LEGACY_SKILL_INSTALL_DIR_NAME),
    ]
}

fn uninstall_legacy_listener_service(
    home_dir: &Path,
    platform: LegacyCleanupPlatform,
    env: &LegacyCleanupEnv,
    runner: &mut dyn FnMut(&str, &[String]) -> Result<(), String>,
) -> Vec<String> {
    match platform {
        LegacyCleanupPlatform::Darwin => uninstall_legacy_listener_service_darwin(home_dir, runner),
        LegacyCleanupPlatform::Linux => {
            uninstall_legacy_listener_service_linux(home_dir, env, runner)
        }
        LegacyCleanupPlatform::Windows => {
            uninstall_legacy_listener_service_windows(home_dir, env, runner)
        }
        LegacyCleanupPlatform::Other => Vec::new(),
    }
}

fn uninstall_legacy_listener_service_darwin(
    home_dir: &Path,
    runner: &mut dyn FnMut(&str, &[String]) -> Result<(), String>,
) -> Vec<String> {
    let plist_path = home_dir
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{LEGACY_MACOS_LISTENER_LABEL}.plist"));
    if !plist_path.is_file() {
        return Vec::new();
    }

    let mut warnings = Vec::new();
    let unload_args = vec!["unload".to_string(), display_path(&plist_path)];
    if let Err(err) = runner("launchctl", &unload_args) {
        warnings.push(format!(
            "Failed to stop legacy awiki listener LaunchAgent: {err}"
        ));
    }
    match fs::remove_file(&plist_path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => warnings.push(format!(
            "Failed to remove legacy awiki listener LaunchAgent plist {}: {err}",
            display_path(&plist_path)
        )),
    }
    warnings
}

fn uninstall_legacy_listener_service_linux(
    home_dir: &Path,
    env: &LegacyCleanupEnv,
    runner: &mut dyn FnMut(&str, &[String]) -> Result<(), String>,
) -> Vec<String> {
    let unit_path = legacy_xdg_config_home(home_dir, env)
        .join("systemd")
        .join("user")
        .join(LEGACY_LINUX_LISTENER_UNIT);
    if !unit_path.is_file() {
        return Vec::new();
    }

    let mut warnings = Vec::new();
    let stop_args = vec![
        "--user".to_string(),
        "disable".to_string(),
        "--now".to_string(),
        LEGACY_LINUX_LISTENER_UNIT.to_string(),
    ];
    if let Err(err) = runner("systemctl", &stop_args) {
        warnings.push(format!(
            "Failed to stop legacy awiki listener systemd user service: {err}"
        ));
    }
    match fs::remove_file(&unit_path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => warnings.push(format!(
            "Failed to remove legacy awiki listener systemd unit {}: {err}",
            display_path(&unit_path)
        )),
    }
    let reload_args = vec!["--user".to_string(), "daemon-reload".to_string()];
    if let Err(err) = runner("systemctl", &reload_args) {
        warnings.push(format!(
            "Failed to reload systemd user units after legacy awiki listener cleanup: {err}"
        ));
    }
    warnings
}

fn uninstall_legacy_listener_service_windows(
    home_dir: &Path,
    env: &LegacyCleanupEnv,
    runner: &mut dyn FnMut(&str, &[String]) -> Result<(), String>,
) -> Vec<String> {
    let app_dir = legacy_local_app_data(home_dir, env).join(LEGACY_WINDOWS_LISTENER_TASK_NAME);
    if !app_dir.exists() {
        return Vec::new();
    }

    let mut warnings = Vec::new();
    let stop_args = vec![
        "/End".to_string(),
        "/TN".to_string(),
        LEGACY_WINDOWS_LISTENER_TASK_NAME.to_string(),
    ];
    if let Err(err) = runner("schtasks", &stop_args) {
        warnings.push(format!(
            "Failed to stop legacy awiki listener scheduled task: {err}"
        ));
    }
    let delete_args = vec![
        "/Delete".to_string(),
        "/TN".to_string(),
        LEGACY_WINDOWS_LISTENER_TASK_NAME.to_string(),
        "/F".to_string(),
    ];
    if let Err(err) = runner("schtasks", &delete_args) {
        warnings.push(format!(
            "Failed to remove legacy awiki listener scheduled task: {err}"
        ));
    }
    if let Err(err) = remove_all(&app_dir) {
        warnings.push(format!(
            "Failed to remove legacy awiki listener app directory {}: {err}",
            display_path(&app_dir)
        ));
    }
    warnings
}

fn remove_legacy_heartbeat_section(home_dir: &Path, env: &LegacyCleanupEnv) -> std::io::Result<()> {
    let workspace_dir = env
        .openclaw_workspace
        .clone()
        .unwrap_or_else(|| home_dir.join(".openclaw").join("workspace"));
    let heartbeat_path = workspace_dir.join("HEARTBEAT.md");
    if !heartbeat_path.is_file() {
        return Ok(());
    }

    let content_bytes = fs::read(&heartbeat_path)
        .map_err(|err| prefix_io(format!("read {}", display_path(&heartbeat_path)), err))?;
    let Some(start_index) = find_bytes(&content_bytes, LEGACY_HEARTBEAT_SECTION_START.as_bytes())
    else {
        return Ok(());
    };
    let Some(mut end_index) = find_bytes(&content_bytes, LEGACY_HEARTBEAT_SECTION_END.as_bytes())
    else {
        return Ok(());
    };
    if end_index < start_index {
        return Ok(());
    }
    end_index += LEGACY_HEARTBEAT_SECTION_END.len();
    let section = &content_bytes[start_index..end_index];
    if find_bytes(section, LEGACY_SKILL_INSTALL_DIR_NAME.as_bytes()).is_none() {
        return Ok(());
    }

    let mut updated = Vec::new();
    updated.extend_from_slice(&content_bytes[..start_index]);
    updated.extend_from_slice(&content_bytes[end_index..]);
    while updated.first() == Some(&b'\n') {
        updated.remove(0);
    }
    let updated = collapse_extra_blank_lines(updated);
    let info = fs::metadata(&heartbeat_path)
        .map_err(|err| prefix_io(format!("stat {}", display_path(&heartbeat_path)), err))?;
    fs::write(&heartbeat_path, updated)?;
    fs::set_permissions(&heartbeat_path, info.permissions())?;
    Ok(())
}

fn collapse_extra_blank_lines(mut content: Vec<u8>) -> Vec<u8> {
    while let Some(index) = find_bytes(&content, b"\n\n\n") {
        content.splice(index..index + 3, *b"\n\n");
    }
    content
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn legacy_xdg_config_home(home_dir: &Path, env: &LegacyCleanupEnv) -> PathBuf {
    env.xdg_config_home
        .clone()
        .unwrap_or_else(|| home_dir.join(".config"))
}

fn legacy_local_app_data(home_dir: &Path, env: &LegacyCleanupEnv) -> PathBuf {
    env.local_app_data
        .clone()
        .unwrap_or_else(|| home_dir.join("AppData").join("Local"))
}

fn remove_all(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn legacy_cleanup_user_home() -> Result<PathBuf, std::io::Error> {
    #[cfg(windows)]
    {
        if let Some(path) = std::env::var_os("USERPROFILE") {
            if !path.as_os_str().is_empty() {
                return Ok(PathBuf::from(path));
            }
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "%userprofile% is not defined",
        ));
    }
    #[cfg(not(windows))]
    {
        if let Some(path) = std::env::var_os("HOME") {
            if !path.as_os_str().is_empty() {
                return Ok(PathBuf::from(path));
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "$HOME is not defined",
        ))
    }
}

fn system_command_runner(name: &str, args: &[String]) -> Result<(), String> {
    let output = Command::new(name)
        .args(args)
        .output()
        .map_err(|err| err.to_string())?;
    if output.status.success() {
        return Ok(());
    }
    let mut combined = output.stdout;
    combined.extend(output.stderr);
    let message = String::from_utf8_lossy(&combined).trim().to_string();
    let status = output
        .status
        .code()
        .map(|code| format!("exit status {code}"))
        .unwrap_or_else(|| output.status.to_string());
    if message.is_empty() {
        Err(status)
    } else {
        Err(format!("{status}: {message}"))
    }
}

fn trimmed_env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).and_then(trimmed_os_path)
}

fn trimmed_os_path(value: std::ffi::OsString) -> Option<PathBuf> {
    let text = value.to_string_lossy().trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(PathBuf::from(text))
    }
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn prefix_io(prefix: String, err: std::io::Error) -> std::io::Error {
    std::io::Error::new(err.kind(), format!("{prefix}: {err}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegacyCleanupPlatform {
    Darwin,
    Linux,
    Windows,
    Other,
}

impl LegacyCleanupPlatform {
    fn current() -> Self {
        Self::from_goos(std::env::consts::OS)
    }

    fn from_goos(goos: &str) -> Self {
        match goos {
            "macos" | "darwin" => Self::Darwin,
            "linux" => Self::Linux,
            "windows" => Self::Windows,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct LegacyCleanupEnv {
    openclaw_workspace: Option<PathBuf>,
    xdg_config_home: Option<PathBuf>,
    local_app_data: Option<PathBuf>,
}

impl LegacyCleanupEnv {
    fn from_process() -> Self {
        Self {
            openclaw_workspace: trimmed_env_path("OPENCLAW_WORKSPACE"),
            xdg_config_home: trimmed_env_path("XDG_CONFIG_HOME"),
            local_app_data: trimmed_env_path("LOCALAPPDATA"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{workspace_config, workspace_upgrade};

    #[test]
    fn cleanup_legacy_skill_artifacts_removes_linux_artifacts_and_heartbeat() {
        let temp = TempDir::new("workspace-v1-v2-linux-cleanup").expect("temp dir");
        let home = temp.path();
        write_legacy_skill_artifacts(
            home,
            LegacyCleanupPlatform::Linux,
            &LegacyCleanupEnv::default(),
        );
        let mut calls = Vec::new();
        let warnings = {
            let mut runner = recording_runner(&mut calls, Ok(()));
            cleanup_legacy_skill_artifacts_for_home(
                home,
                LegacyCleanupPlatform::Linux,
                &LegacyCleanupEnv::default(),
                &mut runner,
            )
        };

        assert!(warnings.is_empty());
        assert_legacy_skill_artifacts_removed(
            home,
            LegacyCleanupPlatform::Linux,
            &LegacyCleanupEnv::default(),
        );
        assert_eq!(
            calls,
            vec![
                "systemctl --user disable --now awiki-ws-listener.service",
                "systemctl --user daemon-reload",
            ]
        );
    }

    #[test]
    fn cleanup_legacy_listener_service_matches_darwin_and_windows_commands() {
        let temp = TempDir::new("workspace-v1-v2-platform-cleanup").expect("temp dir");
        let darwin_home = temp.path().join("darwin-home");
        let windows_home = temp.path().join("windows-home");
        write_service_artifact(
            &darwin_home,
            LegacyCleanupPlatform::Darwin,
            &LegacyCleanupEnv::default(),
        );
        write_service_artifact(
            &windows_home,
            LegacyCleanupPlatform::Windows,
            &LegacyCleanupEnv::default(),
        );

        let mut darwin_calls = Vec::new();
        let darwin_warnings = {
            let mut darwin_runner = recording_runner(&mut darwin_calls, Ok(()));
            uninstall_legacy_listener_service(
                &darwin_home,
                LegacyCleanupPlatform::Darwin,
                &LegacyCleanupEnv::default(),
                &mut darwin_runner,
            )
        };
        assert!(darwin_warnings.is_empty());
        assert_eq!(
            darwin_calls,
            vec![format!(
                "launchctl unload {}",
                display_path(
                    &darwin_home
                        .join("Library")
                        .join("LaunchAgents")
                        .join("com.awiki.ws-listener.plist")
                )
            )]
        );
        assert!(!service_artifact_path(
            &darwin_home,
            LegacyCleanupPlatform::Darwin,
            &LegacyCleanupEnv::default()
        )
        .exists());

        let mut windows_calls = Vec::new();
        let windows_warnings = {
            let mut windows_runner = recording_runner(&mut windows_calls, Ok(()));
            uninstall_legacy_listener_service(
                &windows_home,
                LegacyCleanupPlatform::Windows,
                &LegacyCleanupEnv::default(),
                &mut windows_runner,
            )
        };
        assert!(windows_warnings.is_empty());
        assert_eq!(
            windows_calls,
            vec![
                "schtasks /End /TN awiki-ws-listener",
                "schtasks /Delete /TN awiki-ws-listener /F",
            ]
        );
        assert!(!service_artifact_path(
            &windows_home,
            LegacyCleanupPlatform::Windows,
            &LegacyCleanupEnv::default()
        )
        .exists());
    }

    #[test]
    fn cleanup_legacy_skill_artifacts_records_command_warnings_and_keeps_going() {
        let temp = TempDir::new("workspace-v1-v2-warning-cleanup").expect("temp dir");
        let home = temp.path();
        write_service_artifact(
            home,
            LegacyCleanupPlatform::Linux,
            &LegacyCleanupEnv::default(),
        );
        let mut calls = Vec::new();
        let warnings = {
            let mut runner = recording_runner(&mut calls, Err("exit status 1: denied".to_string()));
            cleanup_legacy_skill_artifacts_for_home(
                home,
                LegacyCleanupPlatform::Linux,
                &LegacyCleanupEnv::default(),
                &mut runner,
            )
        };

        assert_eq!(
            warnings,
            vec![
                "Failed to stop legacy awiki listener systemd user service: exit status 1: denied",
                "Failed to reload systemd user units after legacy awiki listener cleanup: exit status 1: denied",
            ]
        );
        assert_eq!(
            calls,
            vec![
                "systemctl --user disable --now awiki-ws-listener.service",
                "systemctl --user daemon-reload",
            ]
        );
        assert!(!service_artifact_path(
            home,
            LegacyCleanupPlatform::Linux,
            &LegacyCleanupEnv::default()
        )
        .exists());
    }

    #[test]
    fn cleanup_legacy_heartbeat_uses_openclaw_workspace_and_ignores_unrelated_sections() {
        let temp = TempDir::new("workspace-v1-v2-heartbeat").expect("temp dir");
        let home = temp.path().join("home");
        let workspace = temp.path().join("custom-openclaw-workspace");
        let env = LegacyCleanupEnv {
            openclaw_workspace: Some(workspace.clone()),
            ..Default::default()
        };
        fs::create_dir_all(&workspace).expect("create workspace");
        let heartbeat = workspace.join("HEARTBEAT.md");
        fs::write(
            &heartbeat,
            "# Keep\n\n<!-- awiki-heartbeat-start -->\nnot this skill\n<!-- awiki-heartbeat-end -->\n",
        )
        .expect("write heartbeat");

        remove_legacy_heartbeat_section(&home, &env).expect("unrelated section is ignored");
        let unchanged = fs::read_to_string(&heartbeat).expect("read heartbeat");
        assert!(unchanged.contains("not this skill"));

        fs::write(
            &heartbeat,
            "\n\n# Heartbeat checklist\n\n<!-- awiki-heartbeat-start -->\nRun awiki-agent-id-message\n<!-- awiki-heartbeat-end -->\n\n\n## Other checks\n",
        )
        .expect("write matching heartbeat");
        remove_legacy_heartbeat_section(&home, &env).expect("remove section");
        let updated = fs::read_to_string(&heartbeat).expect("read updated heartbeat");
        assert_eq!(updated, "# Heartbeat checklist\n\n## Other checks\n");
    }

    #[test]
    fn apply_workspace_v1_to_v2_cleanup_keeps_go_guard_and_appends_warnings() {
        let missing = apply_workspace_v1_to_v2_cleanup_optional(None)
            .expect_err("missing context should match Go guard");
        assert_eq!(missing.to_string(), "workspace upgrade context is required");

        let temp = TempDir::new("workspace-v1-v2-apply").expect("temp dir");
        let resolved = test_resolved(temp.path());
        let mut context = workspace_upgrade::new_context(&resolved, "1.2.3");
        let home = temp.path().join("home");
        write_service_artifact(
            &home,
            LegacyCleanupPlatform::Linux,
            &LegacyCleanupEnv::default(),
        );
        let mut calls = Vec::new();
        let mut runner = recording_runner(&mut calls, Err("boom".to_string()));

        apply_workspace_v1_to_v2_cleanup_with(
            &mut context,
            &home,
            LegacyCleanupPlatform::Linux,
            &LegacyCleanupEnv::default(),
            &mut runner,
        )
        .expect("apply cleanup");

        assert_eq!(
            context.warnings,
            vec![
                "Failed to stop legacy awiki listener systemd user service: boom",
                "Failed to reload systemd user units after legacy awiki listener cleanup: boom",
            ]
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn cleanup_legacy_skill_artifacts_reports_go_home_error() {
        let _home_guard = EnvGuard::set("HOME", None);

        let warnings = cleanup_legacy_skill_artifacts();

        assert_eq!(
            warnings,
            vec!["Legacy awiki skill cleanup skipped: resolve home directory failed: $HOME is not defined"]
        );
    }

    #[test]
    fn migration_apply_wires_v1_to_v2_cleanup_without_full_upgrade_execution() {
        let temp = TempDir::new("workspace-v1-v2-migration-apply").expect("temp dir");
        let home = temp.path().join("home");
        fs::create_dir_all(&home).expect("create home");
        let _home_guard = EnvGuard::set("HOME", Some(display_path(&home)));
        write_skill_and_heartbeat_only(&home);

        let resolved = test_resolved(temp.path());
        let mut context = workspace_upgrade::new_context(&resolved, "1.2.3");
        let upgrader = workspace_upgrade::new_default_upgrader();
        let plan = upgrader.plan(1, 2).expect("v1 to v2 plan");
        assert_eq!(
            plan[0].name(),
            "workspace_1_to_2_remove_legacy_skill_and_listener"
        );

        plan[0].apply(&mut context).expect("v1 to v2 apply");
        plan[0]
            .validate(&context)
            .expect("v1 to v2 validate is no-op");

        assert!(context.warnings.is_empty());
        for path in legacy_skill_install_dirs(&home) {
            assert!(!path.exists(), "legacy skill path still exists: {path:?}");
        }
        let heartbeat = home
            .join(".openclaw")
            .join("workspace")
            .join("HEARTBEAT.md");
        let updated = fs::read_to_string(heartbeat).expect("read heartbeat");
        assert_eq!(updated, "# Heartbeat checklist\n\n## Other checks\n");
    }

    fn apply_workspace_v1_to_v2_cleanup_with(
        context: &mut Context,
        home_dir: &Path,
        platform: LegacyCleanupPlatform,
        env: &LegacyCleanupEnv,
        runner: &mut dyn FnMut(&str, &[String]) -> Result<(), String>,
    ) -> Result<(), MigrationError> {
        context
            .warnings
            .extend(cleanup_legacy_skill_artifacts_for_home(
                home_dir, platform, env, runner,
            ));
        Ok(())
    }

    fn recording_runner<'a>(
        calls: &'a mut Vec<String>,
        result: Result<(), String>,
    ) -> impl FnMut(&str, &[String]) -> Result<(), String> + 'a {
        move |name, args| {
            let mut command = Vec::with_capacity(args.len() + 1);
            command.push(name.to_string());
            command.extend(args.iter().cloned());
            calls.push(command.join(" "));
            result.clone()
        }
    }

    fn write_legacy_skill_artifacts(
        home: &Path,
        platform: LegacyCleanupPlatform,
        env: &LegacyCleanupEnv,
    ) {
        for path in legacy_skill_install_dirs(home) {
            fs::create_dir_all(&path).expect("create skill dir");
            fs::write(path.join("SKILL.md"), "# legacy\n").expect("write skill");
        }
        write_service_artifact(home, platform, env);
        let heartbeat = home
            .join(".openclaw")
            .join("workspace")
            .join("HEARTBEAT.md");
        fs::create_dir_all(heartbeat.parent().unwrap()).expect("create heartbeat dir");
        let legacy_skill_dir = home
            .join(".openclaw")
            .join("skills")
            .join(LEGACY_SKILL_INSTALL_DIR_NAME);
        let heartbeat_content = format!(
            "# Heartbeat checklist\n\n{LEGACY_HEARTBEAT_SECTION_START}\n## awiki - DID messaging (every heartbeat)\n\n- Run: `cd {} && python scripts/check_status.py`\n{LEGACY_HEARTBEAT_SECTION_END}\n\n## Other checks\n\n- Keep this section.\n",
            display_path(&legacy_skill_dir)
        );
        fs::write(heartbeat, heartbeat_content).expect("write heartbeat");
    }

    fn write_skill_and_heartbeat_only(home: &Path) {
        for path in legacy_skill_install_dirs(home) {
            fs::create_dir_all(&path).expect("create skill dir");
            fs::write(path.join("SKILL.md"), "# legacy\n").expect("write skill");
        }
        let heartbeat = home
            .join(".openclaw")
            .join("workspace")
            .join("HEARTBEAT.md");
        fs::create_dir_all(heartbeat.parent().unwrap()).expect("create heartbeat dir");
        fs::write(
            heartbeat,
            "\n# Heartbeat checklist\n\n<!-- awiki-heartbeat-start -->\nRun awiki-agent-id-message\n<!-- awiki-heartbeat-end -->\n\n\n## Other checks\n",
        )
        .expect("write heartbeat");
    }

    fn write_service_artifact(
        home: &Path,
        platform: LegacyCleanupPlatform,
        env: &LegacyCleanupEnv,
    ) {
        let path = service_artifact_path(home, platform, env);
        fs::create_dir_all(path.parent().unwrap()).expect("create service dir");
        if platform == LegacyCleanupPlatform::Windows {
            fs::create_dir_all(path.parent().unwrap()).expect("create windows app dir");
        }
        fs::write(path, "legacy service\n").expect("write service");
    }

    fn assert_legacy_skill_artifacts_removed(
        home: &Path,
        platform: LegacyCleanupPlatform,
        env: &LegacyCleanupEnv,
    ) {
        for path in legacy_skill_install_dirs(home) {
            assert!(!path.exists(), "legacy skill path still exists: {path:?}");
        }
        assert!(
            !service_artifact_path(home, platform, env).exists(),
            "legacy service artifact still exists"
        );
        let heartbeat = home
            .join(".openclaw")
            .join("workspace")
            .join("HEARTBEAT.md");
        let text = fs::read_to_string(heartbeat).expect("read heartbeat");
        assert!(!text.contains(LEGACY_HEARTBEAT_SECTION_START));
        assert!(!text.contains(LEGACY_SKILL_INSTALL_DIR_NAME));
        assert!(text.contains("## Other checks"));
    }

    fn service_artifact_path(
        home: &Path,
        platform: LegacyCleanupPlatform,
        env: &LegacyCleanupEnv,
    ) -> PathBuf {
        match platform {
            LegacyCleanupPlatform::Darwin => home
                .join("Library")
                .join("LaunchAgents")
                .join("com.awiki.ws-listener.plist"),
            LegacyCleanupPlatform::Linux => legacy_xdg_config_home(home, env)
                .join("systemd")
                .join("user")
                .join(LEGACY_LINUX_LISTENER_UNIT),
            LegacyCleanupPlatform::Windows => legacy_local_app_data(home, env)
                .join(LEGACY_WINDOWS_LISTENER_TASK_NAME)
                .join("run-listener.bat"),
            LegacyCleanupPlatform::Other => home.join("legacy-listener-service"),
        }
    }

    fn test_resolved(root: &Path) -> workspace_config::Resolved {
        workspace_config::Resolved {
            paths: workspace_config::Paths {
                workspace_home_dir: path_string(root),
                root_dir: path_string(root),
                config_dir: path_string(root),
                data_dir: path_string(&root.join("data")),
                state_dir: path_string(&root.join("runtime")),
                cache_dir: path_string(&root.join("cache")),
                logs_dir: path_string(&root.join("logs")),
                config_file: path_string(&root.join("config.yaml")),
                identity_dir: path_string(&root.join("identities")),
                database_file: path_string(&root.join("data").join("awiki-cli.db")),
                legacy_credentials_dir: path_string(&root.join("legacy-credentials")),
                legacy_data_dir: path_string(&root.join("legacy-data")),
            },
            config_schema_version: 1,
            active_identity: String::new(),
            runtime_mode: "websocket".to_string(),
            runtime_socket_path: String::new(),
            runtime_listener_enabled: true,
            runtime_listener_auto_install: true,
            runtime_listener_auto_start: true,
            host_notify_enabled: true,
            host_notify_sink: "log".to_string(),
            host_notify_file_path: String::new(),
            host_notify_openclaw_hook_url: String::new(),
            host_notify_openclaw_agent_id: String::new(),
            host_notify_openclaw_hook_name: String::new(),
            host_notify_hermes_notify_url: String::new(),
            host_notify_hermes_deliver: String::new(),
            output_format: "json".to_string(),
            no_color: false,
            service_base_url: "https://awiki.ai".to_string(),
            did_domain: "awiki.ai".to_string(),
            anp_service_endpoint: "https://awiki.ai/anp-im/rpc".to_string(),
            anp_service_did: "did:wba:awiki.ai".to_string(),
            mail_service_url: "https://awiki.ai".to_string(),
            ca_bundle: String::new(),
            update_disable_strict_version: false,
            update_metadata_cache_ttl_seconds: 86400,
            config_exists: false,
            config_error: String::new(),
            env_hits: Vec::new(),
            sources: Default::default(),
        }
    }

    fn path_string(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(name: &str) -> std::io::Result<Self> {
            let mut path = std::env::temp_dir();
            path.push(format!(
                "awiki-cli-rs2-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&path)?;
            Ok(Self { path })
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    struct EnvGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: Option<String>) -> Self {
            static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
            let guard = ENV_LOCK.lock().expect("env lock");
            let previous = std::env::var_os(key);
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
            Self {
                key,
                previous,
                _guard: guard,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }
}
