use awiki_cli::host_runtime::listener_connect_session::{
    connect_session_bearer_seedings, simulate_connect_session, ConnectSessionAction,
    ConnectSessionClientOutcome, ConnectSessionConnectOutcome, ConnectSessionInputs,
    ConnectSessionLoadOutcome, ConnectSessionPaths, ConnectSessionPathsOutcome,
    ConnectSessionRecord, LISTENER_CONNECT_TIMEOUT,
};
use std::time::Duration;

#[test]
fn connect_session_propagates_identity_load_error_before_other_work() {
    let result = simulate_connect_session(ConnectSessionInputs {
        identity_name: "alice".to_string(),
        service_base_url: "https://awiki.example".to_string(),
        load: ConnectSessionLoadOutcome::Error("load failed".to_string()),
        paths: None,
        ws_client: None,
        connect: None,
    });

    assert_eq!(
        result.actions,
        vec![ConnectSessionAction::LoadIdentity {
            identity_name: "alice".to_string()
        }]
    );
    assert_eq!(result.error.as_deref(), Some("load failed"));
    assert!(result.auth_plan.is_none());
}

#[test]
fn connect_session_rejects_identity_not_ready_before_paths_lookup() {
    let result = simulate_connect_session(ConnectSessionInputs {
        identity_name: "alice".to_string(),
        service_base_url: "https://awiki.example".to_string(),
        load: ConnectSessionLoadOutcome::Loaded(record_with(
            "alice",
            "did:alice",
            "",
            "",
            "jwt-old",
        )),
        paths: None,
        ws_client: None,
        connect: None,
    });

    assert_eq!(
        result.actions,
        vec![
            ConnectSessionAction::LoadIdentity {
                identity_name: "alice".to_string()
            },
            ConnectSessionAction::EvaluateUserState {
                identity_name: "alice".to_string()
            },
        ]
    );
    assert_eq!(
        result.error.as_deref(),
        Some("registered handle user is required: identity alice is local_identity and missing registration, handle; complete user setup with `awiki-cli id register --handle <handle> ...` or recover an existing handle first")
    );
}

#[test]
fn connect_session_uses_active_identity_name_in_registration_error_for_blank_record_name() {
    let result = simulate_connect_session(ConnectSessionInputs {
        identity_name: "active".to_string(),
        service_base_url: "https://awiki.example".to_string(),
        load: ConnectSessionLoadOutcome::Loaded(record_with("", "did:alice", "", "alice", "")),
        paths: None,
        ws_client: None,
        connect: None,
    });

    assert_eq!(
        result.error.as_deref(),
        Some("registered handle user is required: identity active identity is partial_user and missing registration; complete user setup with `awiki-cli id register --handle <handle> ...` or recover an existing handle first")
    );
}

#[test]
fn connect_session_propagates_paths_error_after_readiness_check() {
    let result = simulate_connect_session(ConnectSessionInputs {
        identity_name: "alice".to_string(),
        service_base_url: "https://awiki.example".to_string(),
        load: ConnectSessionLoadOutcome::Loaded(ready_record("jwt-old")),
        paths: Some(ConnectSessionPathsOutcome::Error(
            "paths failed".to_string(),
        )),
        ws_client: None,
        connect: None,
    });

    assert_eq!(
        result.actions,
        vec![
            ConnectSessionAction::LoadIdentity {
                identity_name: "alice".to_string()
            },
            ConnectSessionAction::EvaluateUserState {
                identity_name: "alice".to_string()
            },
            ConnectSessionAction::PathsForIdentity {
                identity_name: "alice".to_string()
            },
        ]
    );
    assert_eq!(result.error.as_deref(), Some("paths failed"));
}

#[test]
fn connect_session_auth_plan_seeds_bearer_scopes_when_stored_jwt_is_nonblank() {
    let result = simulate_connect_session(ConnectSessionInputs {
        identity_name: "alice".to_string(),
        service_base_url: "https://awiki.example/".to_string(),
        load: ConnectSessionLoadOutcome::Loaded(ready_record(" jwt-old ")),
        paths: Some(ConnectSessionPathsOutcome::Loaded(paths())),
        ws_client: Some(ConnectSessionClientOutcome::Error(
            "new ws failed".to_string(),
        )),
        connect: None,
    });

    let auth_plan = result.auth_plan.expect("auth plan");
    assert_eq!(auth_plan.did_document_path, "/ids/alice/did.json");
    assert_eq!(auth_plan.key1_private_path, "/ids/alice/key.pem");
    assert_eq!(auth_plan.identity_name, "alice");
    assert_eq!(auth_plan.did, "did:alice");
    assert_eq!(auth_plan.initial_jwt, " jwt-old ");
    assert_eq!(
        auth_plan
            .bearer_seedings
            .iter()
            .map(|seed| (seed.scope.as_str(), seed.token.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("https://awiki.example/", " jwt-old "),
            (
                "https://awiki.example/user-service/v1/did-auth/rpc",
                " jwt-old "
            ),
            ("https://awiki.example/im/ws", " jwt-old "),
        ]
    );
    assert_eq!(result.error.as_deref(), Some("new ws failed"));
    assert!(!result.actions.contains(&ConnectSessionAction::CloseClient));
}

#[test]
fn connect_session_does_not_seed_bearer_for_blank_stored_jwt() {
    let seeds = connect_session_bearer_seedings("https://awiki.example", " \t\n ");

    assert!(seeds.is_empty());
}

#[test]
fn connect_session_client_error_does_not_close_unconstructed_client() {
    let result = simulate_connect_session(ConnectSessionInputs {
        identity_name: "alice".to_string(),
        service_base_url: "https://awiki.example".to_string(),
        load: ConnectSessionLoadOutcome::Loaded(ready_record("jwt-old")),
        paths: Some(ConnectSessionPathsOutcome::Loaded(paths())),
        ws_client: Some(ConnectSessionClientOutcome::Error(
            "new ws failed".to_string(),
        )),
        connect: None,
    });

    assert_eq!(result.error.as_deref(), Some("new ws failed"));
    assert!(result.actions.contains(&ConnectSessionAction::NewWsClient));
    assert!(!result.actions.contains(&ConnectSessionAction::CloseClient));
    assert!(!result
        .actions
        .contains(&ConnectSessionAction::ConnectWithTimeout {
            timeout: LISTENER_CONNECT_TIMEOUT,
        }));
}

#[test]
fn connect_session_connect_error_closes_constructed_client() {
    let result = simulate_connect_session(ConnectSessionInputs {
        identity_name: "alice".to_string(),
        service_base_url: "https://awiki.example".to_string(),
        load: ConnectSessionLoadOutcome::Loaded(ready_record("jwt-old")),
        paths: Some(ConnectSessionPathsOutcome::Loaded(paths())),
        ws_client: Some(ConnectSessionClientOutcome::Constructed),
        connect: Some(ConnectSessionConnectOutcome::Error(
            "dial failed".to_string(),
        )),
    });

    assert_eq!(
        result.actions.last(),
        Some(&ConnectSessionAction::CloseClient)
    );
    assert!(result
        .actions
        .contains(&ConnectSessionAction::ConnectWithTimeout {
            timeout: Duration::from_secs(15),
        }));
    assert_eq!(result.error.as_deref(), Some("dial failed"));
    assert!(result.returned_record_jwt.is_none());
}

#[test]
fn connect_session_success_updates_record_jwt_from_auth_session_current_jwt() {
    let result = simulate_connect_session(ConnectSessionInputs {
        identity_name: "alice".to_string(),
        service_base_url: "https://awiki.example".to_string(),
        load: ConnectSessionLoadOutcome::Loaded(ready_record("jwt-old")),
        paths: Some(ConnectSessionPathsOutcome::Loaded(paths())),
        ws_client: Some(ConnectSessionClientOutcome::Constructed),
        connect: Some(ConnectSessionConnectOutcome::Connected {
            current_jwt: "jwt-new".to_string(),
        }),
    });

    assert_eq!(result.error, None);
    assert_eq!(result.returned_record_jwt.as_deref(), Some("jwt-new"));
    assert_eq!(
        result.actions.last_chunk::<2>(),
        Some(&[
            ConnectSessionAction::UpdateRecordJwt {
                jwt_token: "jwt-new".to_string()
            },
            ConnectSessionAction::ReturnConnected,
        ])
    );
}

fn ready_record(jwt_token: &str) -> ConnectSessionRecord {
    record_with("alice", "did:alice", "user-1", "alice", jwt_token)
}

fn record_with(
    identity_name: &str,
    did: &str,
    user_id: &str,
    handle: &str,
    jwt_token: &str,
) -> ConnectSessionRecord {
    ConnectSessionRecord {
        identity_name: identity_name.to_string(),
        did: did.to_string(),
        user_id: user_id.to_string(),
        handle: handle.to_string(),
        jwt_token: jwt_token.to_string(),
    }
}

fn paths() -> ConnectSessionPaths {
    ConnectSessionPaths {
        did_document_path: "/ids/alice/did.json".to_string(),
        key1_private_path: "/ids/alice/key.pem".to_string(),
    }
}
