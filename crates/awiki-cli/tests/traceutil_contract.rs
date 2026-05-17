use awiki_cli::traceutil::{self, Run};
use std::path::Path;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn trace_timing_env_truthy_values_match_go_contract() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    for value in ["1", "true", "TRUE", " yes ", "on"] {
        std::env::set_var("AWIKI_CLI_TRACE_TIMING", value);
        assert!(traceutil::enabled(), "{value:?} should enable tracing");
    }
    for value in ["", "0", "false", "no", "off", "enabled"] {
        std::env::set_var("AWIKI_CLI_TRACE_TIMING", value);
        assert!(!traceutil::enabled(), "{value:?} should disable tracing");
    }
    std::env::remove_var("AWIKI_CLI_TRACE_TIMING");
    assert!(!traceutil::enabled(), "unset env should disable tracing");
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
    let mut done = run.rpc_phase("POST /user-service/auth/email-send");
    done.finish();

    let output = run.emit_to_string().expect("emit trace");
    assert!(
        output.contains("远端 RPC / POST user-service auth email-send"),
        "pretty output did not humanize phase name:\n{output}"
    );
    assert_eq!(
        traceutil::phase_name("business_rpc", "POST /user-service/auth/email-send"),
        "business_rpc:POST_user-service.auth.email-send"
    );
    assert_eq!(
        traceutil::humanize_text("read_history_cache"),
        "读取历史缓存"
    );
    assert_eq!(
        traceutil::humanize_text("custom.stage_name"),
        "custom stage name"
    );
    assert_eq!(traceutil::humanize_phase_name(""), "未命名阶段");
    assert_eq!(traceutil::phase_group_label("local_db"), "本地数据库");
    assert_eq!(traceutil::phase_group_label("ensure_jwt"), "JWT 续期");
    assert_eq!(traceutil::phase_group_label("handle_lookup"), "Handle 解析");
    assert_eq!(traceutil::phase_group_label("update_check"), "检查更新");
    assert_eq!(
        traceutil::phase_group_label("update_registry_fetch"),
        "请求更新源"
    );
    assert_eq!(traceutil::phase_group_label("bridge_call"), "调用本地桥");
    assert_eq!(traceutil::format_duration_ms(999), "999 毫秒");
    assert_eq!(traceutil::format_duration_ms(1500), "1.500 秒");
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
fn direct_message_trace_call_sites_match_go_trace_depth_contract() {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let service = source(crate_root, "src/message/service.rs");
    let inbox = source(crate_root, "src/message/inbox.rs");
    let history = source(crate_root, "src/message/history.rs");
    let contact_sync = source(crate_root, "src/message/contact_sync.rs");

    for (label, source) in [
        (
            "traceutil::handle_lookup_phase(\"target_resolve\")",
            &service,
        ),
        (
            "traceutil::handle_lookup_phase(\"contact_sync_by_did\")",
            &contact_sync,
        ),
        (
            "traceutil::local_db_phase(\"persist_direct_send\")",
            &service,
        ),
        (
            "traceutil::local_db_phase(\"persist_inbox_messages\")",
            &service,
        ),
        (
            "traceutil::local_db_phase(\"persist_history_messages\")",
            &service,
        ),
        ("traceutil::local_db_phase(\"read_inbox_cache\")", &inbox),
        (
            "traceutil::local_db_phase(\"read_unified_direct_inbox_cache\")",
            &inbox,
        ),
        (
            "traceutil::local_db_phase(\"read_mail_notification_cache\")",
            &inbox,
        ),
        (
            "traceutil::local_db_phase(\"read_history_cache\")",
            &history,
        ),
        (
            "traceutil::local_db_phase(\"read_inbox_cache_by_peer_dids\")",
            &inbox,
        ),
        (
            "traceutil::local_db_phase(\"read_history_cache_by_peer_dids\")",
            &history,
        ),
    ] {
        assert!(
            source.contains(label),
            "direct-message Go trace label is not wired: {label}"
        );
    }

    assert_eq!(
        service
            .matches("traceutil::start_phase(\"contact_sync\")")
            .count(),
        2,
        "inbox and history persistence should each wrap only contact sync"
    );
    assert_eq!(
        traceutil::humanize_text("read_unified_direct_inbox_cache"),
        "读取统一直聊收件箱缓存"
    );
}

#[test]
fn group_trace_call_sites_match_go_trace_depth_contract() {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let group_service = source(crate_root, "src/message/group_service.rs");
    let inbox = source(crate_root, "src/message/inbox.rs");

    for (label, source) in [
        (
            "traceutil::local_db_phase(\"persist_group_send\")",
            &group_service,
        ),
        (
            "traceutil::local_db_phase(\"persist_group_snapshot\")",
            &group_service,
        ),
        (
            "traceutil::local_db_phase(\"persist_group_members\")",
            &group_service,
        ),
        (
            "traceutil::local_db_phase(\"persist_group_messages\")",
            &group_service,
        ),
        (
            "traceutil::local_db_phase(\"touch_group_cache\")",
            &group_service,
        ),
        (
            "traceutil::local_db_phase(\"mark_group_left\")",
            &group_service,
        ),
        (
            "traceutil::local_db_phase(\"read_group_snapshot_cache\")",
            &group_service,
        ),
        (
            "traceutil::local_db_phase(\"read_group_members_cache\")",
            &group_service,
        ),
        (
            "traceutil::local_db_phase(\"read_group_messages_cache\")",
            &group_service,
        ),
        (
            "traceutil::local_db_phase(\"read_group_inbox_cache\")",
            &inbox,
        ),
        (
            "traceutil::local_db_phase(\"read_all_group_inbox_cache\")",
            &inbox,
        ),
    ] {
        assert!(
            source.contains(label),
            "group Go trace label is not wired: {label}"
        );
    }

    assert_eq!(
        traceutil::humanize_text("read_all_group_inbox_cache"),
        "读取全部群组收件箱缓存"
    );
}

#[test]
fn attachment_trace_call_sites_match_go_trace_depth_contract() {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let attachment_service = source(crate_root, "src/message/attachment_service.rs");

    for label in [
        "traceutil::local_db_phase(\"persist_direct_send\")",
        "traceutil::local_db_phase(\"persist_group_send\")",
        "traceutil::local_db_phase(\"touch_group_cache\")",
    ] {
        assert!(
            attachment_service.contains(label),
            "attachment Go trace label is not wired: {label}"
        );
    }

    for call_site in [
        "let target = resolve_target(resolved, &request.target)?;",
        "let peer = resolve_target(resolved, &request.with)?;",
    ] {
        assert!(
            attachment_service.contains(call_site),
            "attachment direct path should keep shared target_resolve lookup: {call_site}"
        );
    }
}

fn source(crate_root: &Path, relative: &str) -> String {
    std::fs::read_to_string(crate_root.join(relative)).expect("read Rust source")
}
