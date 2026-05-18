use serde_json::Value;
use std::process::{Command, Output};

#[test]
fn schema_flags_omit_empty_fields_like_go_catalog() {
    let config = schema_for(&["config", "set"]);
    let did_domain = schema_flag(schema_command(&config), "did-domain");

    assert_eq!(did_domain["name"], "did-domain");
    assert_eq!(did_domain["type"], "string");
    assert!(did_domain.get("default").is_none());
    assert!(did_domain.get("required").is_none());
    assert!(did_domain.get("choices").is_none());
    assert!(did_domain.get("deprecated").is_none());

    let create = schema_for(&["id", "create"]);
    let name = schema_flag(schema_command(&create), "name");
    assert_eq!(name["required"], true);
    assert!(name.get("default").is_none());
    assert!(name.get("choices").is_none());
    assert!(name.get("deprecated").is_none());

    let msg_send = schema_for(&["msg", "send"]);
    let secure = schema_flag(schema_command(&msg_send), "secure");
    assert_eq!(secure["default"], "off");
    assert_eq!(secure["choices"], serde_json::json!(["off", "on"]));
    assert!(secure.get("required").is_none());
    assert!(secure.get("deprecated").is_none());
}

#[test]
fn schema_command_list_preserves_go_catalog_order_for_drift_prone_groups() {
    let output = awiki_cmd(&["schema"]);
    assert_success(&output);
    let envelope = success_json(&output);
    let commands = envelope["data"]["commands"]
        .as_array()
        .expect("data.commands should be an array");
    let names: Vec<&str> = commands
        .iter()
        .map(|command| {
            command["name"]
                .as_str()
                .expect("schema command should have a string name")
        })
        .collect();

    assert_subsequence(
        &names,
        &[
            "id.use",
            "id.profile",
            "id.profile.get",
            "id.profile.set",
            "id.import-v1",
            "msg",
        ],
    );
    assert_subsequence(
        &names,
        &[
            "runtime.mode.set",
            "runtime.listener",
            "runtime.listener.status",
            "runtime.listener.install",
            "runtime.listener.start",
            "runtime.listener.stop",
            "runtime.listener.restart",
            "runtime.listener.uninstall",
            "runtime.listener.config",
            "runtime.listener.config.show",
            "runtime.listener.config.set",
            "runtime.listener.enable",
            "runtime.listener.disable",
            "runtime.host-notify",
            "runtime.host-notify.config",
            "runtime.host-notify.config.show",
            "runtime.host-notify.config.set",
            "runtime.host-notify.enable",
            "runtime.host-notify.disable",
            "runtime.host-notify.openclaw",
            "runtime.host-notify.openclaw.set",
            "runtime.host-notify.openclaw.set-token",
            "runtime.host-notify.openclaw.clear-token",
            "runtime.host-notify.openclaw.route",
            "runtime.host-notify.openclaw.route.add",
            "runtime.host-notify.openclaw.route.list",
            "runtime.host-notify.openclaw.route.remove",
            "runtime.host-notify.hermes",
            "runtime.host-notify.hermes.guide",
            "runtime.host-notify.hermes.status",
            "runtime.host-notify.hermes.setup",
            "runtime.host-notify.hermes.bridge",
            "runtime.host-notify.hermes.bridge.service-run",
            "runtime.host-notify.hermes.set",
            "runtime.host-notify.hermes.set-secret",
            "runtime.host-notify.hermes.clear-secret",
            "runtime.heartbeat",
            "runtime.heartbeat.status",
            "runtime.heartbeat.install",
            "runtime.heartbeat.run-once",
            "people",
        ],
    );
    assert_subsequence(
        &names,
        &[
            "debug",
            "debug.db",
            "debug.db.handle-history",
            "debug.db.query",
            "debug.db.import-v1",
            "debug.raw",
        ],
    );
}

fn schema_for(path: &[&str]) -> Value {
    let mut args = vec!["schema"];
    args.extend_from_slice(path);
    let output = awiki_cmd(&args);
    assert_success(&output);
    success_json(&output)
}

fn schema_command(schema: &Value) -> &Value {
    &schema["data"]["command"]
}

fn schema_flag<'a>(command: &'a Value, name: &str) -> &'a Value {
    command["flags"]
        .as_array()
        .expect("schema flags should be an array")
        .iter()
        .find(|flag| flag["name"] == name)
        .unwrap_or_else(|| panic!("missing schema flag {name:?}: {command:?}"))
}

fn assert_subsequence(actual: &[&str], expected: &[&str]) {
    let mut cursor = 0usize;
    for expected_name in expected {
        let offset = actual[cursor..]
            .iter()
            .position(|actual_name| actual_name == expected_name)
            .unwrap_or_else(|| panic!("missing {expected_name:?} after {cursor}: {actual:?}"));
        cursor += offset + 1;
    }
}

fn awiki_cmd(args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT")
        .env_remove("AWIKI_CLI_TRACE_TIMING");
    command.output().expect("run awiki-cli binary")
}

fn assert_success(output: &Output) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn success_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|err| panic!("stdout should be JSON: {err}; output={output:?}"))
}
