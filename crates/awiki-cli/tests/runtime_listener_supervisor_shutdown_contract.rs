use awiki_cli::runtime::listener_supervisor_shutdown::{
    supervisor_shutdown_plan, ShutdownSession, SupervisorShutdownAction, SupervisorShutdownDecision,
};

#[test]
fn shutdown_cancels_sessions_with_cancel_func_and_closes_clients_under_lock() {
    let plan = supervisor_shutdown_plan(
        &[
            session("alice", true),
            session("bob", false),
            session("carol", true),
        ],
        false,
        None,
        false,
        None,
        false,
        None,
    );

    assert_eq!(
        plan.actions,
        vec![
            SupervisorShutdownAction::LockSessions,
            SupervisorShutdownAction::CancelSession {
                identity_name: "alice".to_string(),
            },
            SupervisorShutdownAction::CloseCurrentClient {
                identity_name: "alice".to_string(),
            },
            SupervisorShutdownAction::CloseCurrentClient {
                identity_name: "bob".to_string(),
            },
            SupervisorShutdownAction::CancelSession {
                identity_name: "carol".to_string(),
            },
            SupervisorShutdownAction::CloseCurrentClient {
                identity_name: "carol".to_string(),
            },
            SupervisorShutdownAction::UnlockSessions,
        ]
    );
    assert_eq!(plan.decision, SupervisorShutdownDecision::ReturnOk);
}

#[test]
fn shutdown_closes_listener_host_notify_then_database_after_sessions() {
    let plan = supervisor_shutdown_plan(
        &[session("alice", true)],
        true,
        None,
        true,
        None,
        true,
        None,
    );

    assert_eq!(
        plan.actions,
        vec![
            SupervisorShutdownAction::LockSessions,
            SupervisorShutdownAction::CancelSession {
                identity_name: "alice".to_string(),
            },
            SupervisorShutdownAction::CloseCurrentClient {
                identity_name: "alice".to_string(),
            },
            SupervisorShutdownAction::UnlockSessions,
            SupervisorShutdownAction::CloseListener,
            SupervisorShutdownAction::CloseHostNotify,
            SupervisorShutdownAction::CloseDatabase,
        ]
    );
    assert_eq!(plan.decision, SupervisorShutdownDecision::ReturnOk);
}

#[test]
fn listener_and_host_notify_close_errors_are_ignored() {
    let plan = supervisor_shutdown_plan(
        &[],
        true,
        Some("listener close failed"),
        true,
        Some("host close failed"),
        false,
        None,
    );

    assert_eq!(
        plan.actions,
        vec![
            SupervisorShutdownAction::LockSessions,
            SupervisorShutdownAction::UnlockSessions,
            SupervisorShutdownAction::CloseListener,
            SupervisorShutdownAction::CloseHostNotify,
        ]
    );
    assert_eq!(
        plan.ignored_listener_error.as_deref(),
        Some("listener close failed")
    );
    assert_eq!(
        plan.ignored_host_notify_error.as_deref(),
        Some("host close failed")
    );
    assert_eq!(plan.decision, SupervisorShutdownDecision::ReturnOk);
}

#[test]
fn database_close_error_is_returned_after_other_resources() {
    let plan = supervisor_shutdown_plan(
        &[],
        true,
        Some("ignored listener"),
        true,
        Some("ignored host"),
        true,
        Some("database close failed"),
    );

    assert_eq!(
        plan.actions,
        vec![
            SupervisorShutdownAction::LockSessions,
            SupervisorShutdownAction::UnlockSessions,
            SupervisorShutdownAction::CloseListener,
            SupervisorShutdownAction::CloseHostNotify,
            SupervisorShutdownAction::CloseDatabase,
        ]
    );
    assert_eq!(
        plan.decision,
        SupervisorShutdownDecision::ReturnDatabaseError("database close failed".to_string())
    );
}

#[test]
fn nil_listener_host_notify_and_database_are_skipped() {
    let plan = supervisor_shutdown_plan(
        &[],
        false,
        Some("unused"),
        false,
        Some("unused"),
        false,
        Some("unused"),
    );

    assert_eq!(
        plan.actions,
        vec![
            SupervisorShutdownAction::LockSessions,
            SupervisorShutdownAction::UnlockSessions,
        ]
    );
    assert_eq!(plan.ignored_listener_error, None);
    assert_eq!(plan.ignored_host_notify_error, None);
    assert_eq!(plan.decision, SupervisorShutdownDecision::ReturnOk);
}

fn session(identity_name: &str, has_cancel: bool) -> ShutdownSession {
    ShutdownSession {
        identity_name: identity_name.to_string(),
        has_cancel,
    }
}
