use awiki_cli::runtime::listener_session_bootstrap::{
    ensure_session_bootstrap_plan, CurrentIdentityLookup, EnsureSessionAction,
    EnsureSessionDecision, EnsureSessionReturn, SessionBootstrapOutcome, SESSION_BOOTSTRAP_TIMEOUT,
};
use std::time::Duration;

#[test]
fn requested_identity_is_trimmed_before_session_lookup() {
    let existing = vec!["alice".to_string()];

    let plan = ensure_session_bootstrap_plan(
        " alice ",
        &existing,
        CurrentIdentityLookup::Error("should not call current".to_string()),
        SessionBootstrapOutcome::Timeout,
    );

    assert_eq!(plan.identity_name, "alice");
    assert_eq!(
        plan.actions,
        vec![EnsureSessionAction::CheckExistingSession {
            identity_name: "alice".to_string(),
        }]
    );
    assert_eq!(
        plan.decision,
        EnsureSessionDecision::ReturnOk {
            session: EnsureSessionReturn::Existing {
                identity_name: "alice".to_string(),
            },
        }
    );
}

#[test]
fn blank_identity_uses_current_identity_name_without_retrimming() {
    let plan = ensure_session_bootstrap_plan(
        "   ",
        &[],
        CurrentIdentityLookup::Ok {
            identity_name: " current identity ".to_string(),
        },
        SessionBootstrapOutcome::InitialOk,
    );

    assert_eq!(plan.identity_name, " current identity ");
    assert_eq!(
        plan.actions,
        vec![
            EnsureSessionAction::ResolveCurrentIdentity,
            EnsureSessionAction::CheckExistingSession {
                identity_name: " current identity ".to_string(),
            },
            EnsureSessionAction::CreateSession {
                identity_name: " current identity ".to_string(),
            },
            EnsureSessionAction::InsertSession {
                identity_name: " current identity ".to_string(),
            },
            EnsureSessionAction::StartRunSessionLoop {
                identity_name: " current identity ".to_string(),
            },
            EnsureSessionAction::WaitInitialResult {
                identity_name: " current identity ".to_string(),
                timeout: Duration::from_secs(15),
            },
        ]
    );
    assert_eq!(
        plan.decision,
        EnsureSessionDecision::ReturnOk {
            session: EnsureSessionReturn::New {
                identity_name: " current identity ".to_string(),
            },
        }
    );
}

#[test]
fn current_identity_error_returns_without_session_lookup() {
    let plan = ensure_session_bootstrap_plan(
        "",
        &[],
        CurrentIdentityLookup::Error("identity not found".to_string()),
        SessionBootstrapOutcome::InitialOk,
    );

    assert_eq!(plan.identity_name, "");
    assert_eq!(
        plan.actions,
        vec![EnsureSessionAction::ResolveCurrentIdentity]
    );
    assert_eq!(
        plan.decision,
        EnsureSessionDecision::ReturnError {
            session: EnsureSessionReturn::None,
            error: "identity not found".to_string(),
        }
    );
}

#[test]
fn existing_session_returns_without_creating_or_starting_loop() {
    let existing = vec!["bob".to_string()];

    let plan = ensure_session_bootstrap_plan(
        "bob",
        &existing,
        CurrentIdentityLookup::NotNeeded,
        SessionBootstrapOutcome::InitialError("should not wait".to_string()),
    );

    assert_eq!(
        plan.actions,
        vec![EnsureSessionAction::CheckExistingSession {
            identity_name: "bob".to_string(),
        }]
    );
    assert_eq!(
        plan.decision,
        EnsureSessionDecision::ReturnOk {
            session: EnsureSessionReturn::Existing {
                identity_name: "bob".to_string(),
            },
        }
    );
}

#[test]
fn new_session_is_inserted_started_and_waited_before_success_return() {
    let plan = ensure_session_bootstrap_plan(
        "carol",
        &[],
        CurrentIdentityLookup::NotNeeded,
        SessionBootstrapOutcome::InitialOk,
    );

    assert_eq!(
        plan.actions,
        vec![
            EnsureSessionAction::CheckExistingSession {
                identity_name: "carol".to_string(),
            },
            EnsureSessionAction::CreateSession {
                identity_name: "carol".to_string(),
            },
            EnsureSessionAction::InsertSession {
                identity_name: "carol".to_string(),
            },
            EnsureSessionAction::StartRunSessionLoop {
                identity_name: "carol".to_string(),
            },
            EnsureSessionAction::WaitInitialResult {
                identity_name: "carol".to_string(),
                timeout: SESSION_BOOTSTRAP_TIMEOUT,
            },
        ]
    );
    assert_eq!(
        plan.decision,
        EnsureSessionDecision::ReturnOk {
            session: EnsureSessionReturn::New {
                identity_name: "carol".to_string(),
            },
        }
    );
}

#[test]
fn init_error_returns_the_new_session_and_original_error() {
    let plan = ensure_session_bootstrap_plan(
        "dave",
        &[],
        CurrentIdentityLookup::NotNeeded,
        SessionBootstrapOutcome::InitialError("auth failed".to_string()),
    );

    assert_eq!(
        plan.decision,
        EnsureSessionDecision::ReturnError {
            session: EnsureSessionReturn::New {
                identity_name: "dave".to_string(),
            },
            error: "auth failed".to_string(),
        }
    );
}

#[test]
fn timeout_returns_the_new_session_and_go_error_text() {
    let plan = ensure_session_bootstrap_plan(
        "erin",
        &[],
        CurrentIdentityLookup::NotNeeded,
        SessionBootstrapOutcome::Timeout,
    );

    assert_eq!(SESSION_BOOTSTRAP_TIMEOUT, Duration::from_secs(15));
    assert_eq!(
        plan.decision,
        EnsureSessionDecision::ReturnError {
            session: EnsureSessionReturn::New {
                identity_name: "erin".to_string(),
            },
            error: "websocket session bootstrap timed out for identity erin".to_string(),
        }
    );
}
