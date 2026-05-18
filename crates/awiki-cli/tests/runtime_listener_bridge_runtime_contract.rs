use awiki_cli::runtime::host_notify_sink::{
    HostNotifySinkImpl, LogHostNotifySink, NoopHostNotifySink,
};
use awiki_cli::runtime::listener_bridge_runtime::{
    bridge_session_bootstrap_signal_pair, bridge_session_bootstrap_timeout_error,
    host_notify_for_bridge_created_session, wait_for_bridge_session_bootstrap_with_timeout,
    BridgeSessionBootstrapResult,
};
use std::sync::Arc;
use std::time::Duration;

#[test]
fn bridge_created_sessions_reuse_supervisor_host_notify_sink_like_go() {
    let supervisor_sink = Arc::new(HostNotifySinkImpl::Log(LogHostNotifySink));

    let session_sink = host_notify_for_bridge_created_session(&supervisor_sink);

    assert!(Arc::ptr_eq(&session_sink, &supervisor_sink));
    assert!(matches!(&*session_sink, HostNotifySinkImpl::Log(_)));
    assert_eq!(Arc::strong_count(&supervisor_sink), 2);
}

#[test]
fn bridge_created_sessions_do_not_replace_enabled_sink_with_fresh_noop() {
    let supervisor_sink = Arc::new(HostNotifySinkImpl::Noop(NoopHostNotifySink));

    let session_sink = host_notify_for_bridge_created_session(&supervisor_sink);

    assert!(Arc::ptr_eq(&session_sink, &supervisor_sink));
}

#[test]
fn bridge_session_bootstrap_wait_returns_initial_success_like_go_signal_initial() {
    let (signal, waiter) = bridge_session_bootstrap_signal_pair("alice");
    signal.signal_success();

    let result = wait_for_bridge_session_bootstrap_with_timeout(waiter, Duration::from_secs(1));

    assert_eq!(result, BridgeSessionBootstrapResult::Connected);
}

#[test]
fn bridge_session_bootstrap_wait_returns_initial_error_like_go_signal_initial() {
    let (signal, waiter) = bridge_session_bootstrap_signal_pair("alice");
    signal.signal_error("auth failed");

    let result = wait_for_bridge_session_bootstrap_with_timeout(waiter, Duration::from_secs(1));

    assert_eq!(
        result,
        BridgeSessionBootstrapResult::InitialError("auth failed".to_string())
    );
}

#[test]
fn bridge_session_bootstrap_wait_times_out_with_go_error_text() {
    let (_signal, waiter) = bridge_session_bootstrap_signal_pair("carol");

    let result = wait_for_bridge_session_bootstrap_with_timeout(waiter, Duration::from_millis(1));

    assert_eq!(
        result,
        BridgeSessionBootstrapResult::Timeout(
            "websocket session bootstrap timed out for identity carol".to_string(),
        )
    );
    assert_eq!(
        bridge_session_bootstrap_timeout_error("carol"),
        "websocket session bootstrap timed out for identity carol"
    );
}
