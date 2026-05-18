use awiki_cli::runtime::host_notify_sink::{
    HostNotifySinkImpl, LogHostNotifySink, NoopHostNotifySink,
};
use awiki_cli::runtime::listener_bridge_runtime::host_notify_for_bridge_created_session;
use std::sync::Arc;

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
