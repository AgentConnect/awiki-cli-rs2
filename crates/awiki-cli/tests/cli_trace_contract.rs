use awiki_cli::cli_trace::{self, Run};
use std::path::Path;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn trace_timing_env_truthy_values_match_go_contract() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    for value in ["1", "true", "TRUE", " yes ", "on"] {
        std::env::set_var("AWIKI_CLI_TRACE_TIMING", value);
        assert!(cli_trace::enabled(), "{value:?} should enable tracing");
    }
    for value in ["", "0", "false", "no", "off", "enabled"] {
        std::env::set_var("AWIKI_CLI_TRACE_TIMING", value);
        assert!(!cli_trace::enabled(), "{value:?} should disable tracing");
    }
    std::env::remove_var("AWIKI_CLI_TRACE_TIMING");
    assert!(!cli_trace::enabled(), "unset env should disable tracing");
}

#[test]
fn trace_run_emit_pretty_format_matches_go_contract() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    std::env::set_var("AWIKI_CLI_TRACE_TIMING", "1");
    let run = Run::new(" awiki-cli test ");
    let mut outer = run.start_phase("outer");
    let mut inner = run.start_phase("inner");
    inner.finish();
    outer.finish();
    let first_outer_duration = run.phases()[0].duration_ms;
    outer.finish();
    assert!(
        run.phases()[0].duration_ms >= first_outer_duration,
        "Go finish closure can be called repeatedly and update the phase duration"
    );
    run.mark_fallback("websocket_to_http", None);

    let output = run.emit_to_string().expect("emit trace");
    for want in [
        "[awiki-cli 耗时追踪]\n",
        "命令: awiki-cli test\n",
        "总耗时: ",
        "阶段:\n",
        "回退:\n",
        "  - WebSocket 降级到 HTTP\n",
        "说明: 各阶段按开始顺序展示，阶段之间可能重叠，因此不会等于总耗时相加。\n",
    ] {
        assert!(output.contains(want), "output missing {want:?}:\n{output}");
    }
    let outer_index = output.find("1. outer:").expect("outer phase");
    let inner_index = output.find("2. inner:").expect("inner phase");
    assert!(
        outer_index < inner_index,
        "phase order not preserved:\n{output}"
    );
}

#[test]
fn trace_phase_detail_sanitizing_and_humanizing_match_go_contract() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    std::env::set_var("AWIKI_CLI_TRACE_TIMING", "1");
    let run = Run::new("awiki-cli test");
    let mut done = run.rpc_phase("POST /user-service/v1/auth/email-send");
    done.finish();

    let output = run.emit_to_string().expect("emit trace");
    assert!(
        output.contains("远端 RPC / POST user-service auth email-send"),
        "pretty output did not humanize phase name:\n{output}"
    );
    assert_eq!(
        cli_trace::phase_name("business_rpc", "POST /user-service/v1/auth/email-send"),
        "business_rpc:POST_user-service.auth.email-send"
    );
    assert_eq!(
        cli_trace::humanize_text("read_history_cache"),
        "读取历史缓存"
    );
    assert_eq!(
        cli_trace::humanize_text("custom.stage_name"),
        "custom stage name"
    );
    assert_eq!(cli_trace::humanize_phase_name(""), "未命名阶段");
    assert_eq!(cli_trace::phase_group_label("local_db"), "本地数据库");
    assert_eq!(cli_trace::phase_group_label("ensure_jwt"), "JWT 续期");
    assert_eq!(cli_trace::phase_group_label("handle_lookup"), "Handle 解析");
    assert_eq!(cli_trace::phase_group_label("update_check"), "检查更新");
    assert_eq!(
        cli_trace::phase_group_label("update_registry_fetch"),
        "请求更新源"
    );
    assert_eq!(cli_trace::phase_group_label("bridge_call"), "调用本地桥");
    assert_eq!(cli_trace::format_duration_ms(999), "999 毫秒");
    assert_eq!(cli_trace::format_duration_ms(1500), "1.500 秒");
}

#[test]
fn trace_fallback_with_cause_formats_cause_in_parentheses() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    std::env::set_var("AWIKI_CLI_TRACE_TIMING", "1");
    let run = Run::new("awiki-cli test");
    run.mark_fallback(" websocket_to_http ", Some(" dial tcp timeout "));

    let output = run.emit_to_string().expect("emit trace");
    assert!(
        output.contains("  - WebSocket 降级到 HTTP ( dial tcp timeout )\n"),
        "fallback cause missing:\n{output}"
    );
}

#[test]
fn trace_disabled_emits_nothing_and_records_no_phases() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    std::env::set_var("AWIKI_CLI_TRACE_TIMING", "0");
    let run = Run::new("awiki-cli test");
    let mut done = run.local_db_phase("read_mail_notifications");
    done.finish();
    run.mark_fallback("websocket_to_http", Some("network down"));

    assert_eq!(run.phases(), Vec::new());
    assert_eq!(run.fallbacks(), Vec::new());
    assert_eq!(run.emit_to_string().expect("emit disabled"), "");
}

#[test]
fn message_cutover_trace_call_sites_stay_in_thin_adapter() {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let adapter = source(crate_root, "src/m_core_cli_adapter/messages.rs");

    assert!(
        adapter.contains("cli_trace::rpc_phase(sdk_send_trace_operation(&request))"),
        "message sends should keep a thin adapter RPC phase"
    );
    assert!(
        adapter.contains("cli_trace::rpc_phase(\"sync.v2.foreground_reconcile\")"),
        "message inbox should trace its foreground v2 reconciliation"
    );
    assert!(
        !adapter.contains("\"message_fallback_refresh\""),
        "im-core message adapter should not create a legacy fallback-refresh trace phase"
    );
    assert!(
        !adapter.contains("handle_lookup_phase"),
        "message adapter should not do direct handle-resolution tracing"
    );
    assert!(
        !adapter.contains("local_db_phase"),
        "message adapter should not do local projection tracing"
    );
}

#[test]
fn legacy_message_trace_labels_remain_humanized_until_final_cleanup() {
    assert_eq!(
        cli_trace::humanize_text("read_unified_direct_inbox_cache"),
        "读取统一直聊收件箱缓存"
    );
    assert_eq!(
        cli_trace::humanize_text("read_all_group_inbox_cache"),
        "读取全部群组收件箱缓存"
    );

    assert_eq!(
        cli_trace::humanize_text("message_fallback_refresh"),
        "消息回退时刷新 JWT"
    );
    assert_eq!(
        cli_trace::humanize_text("message_service_retry"),
        "消息服务重试前刷新 JWT"
    );
}

fn source(crate_root: &Path, relative: &str) -> String {
    std::fs::read_to_string(crate_root.join(relative)).expect("read Rust source")
}
