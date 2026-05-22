use std::time::Duration;

pub const LISTENER_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectSessionRecord {
    pub identity_name: String,
    pub did: String,
    pub user_id: String,
    pub handle: String,
    pub jwt_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectSessionPaths {
    pub did_document_path: String,
    pub key1_private_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectSessionLoadOutcome {
    Loaded(ConnectSessionRecord),
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectSessionPathsOutcome {
    Loaded(ConnectSessionPaths),
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectSessionClientOutcome {
    Constructed,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectSessionConnectOutcome {
    Connected { current_jwt: String },
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectSessionInputs {
    pub identity_name: String,
    pub service_base_url: String,
    pub load: ConnectSessionLoadOutcome,
    pub paths: Option<ConnectSessionPathsOutcome>,
    pub ws_client: Option<ConnectSessionClientOutcome>,
    pub connect: Option<ConnectSessionConnectOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectSessionAction {
    LoadIdentity { identity_name: String },
    EvaluateUserState { identity_name: String },
    PathsForIdentity { identity_name: String },
    NewAuthSession { identity_name: String },
    SeedBearer { scope: String, token: String },
    NewWsClient,
    ConnectWithTimeout { timeout: Duration },
    CloseClient,
    UpdateRecordJwt { jwt_token: String },
    ReturnConnected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectSessionBearerSeed {
    pub scope: String,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectSessionAuthPlan {
    pub did_document_path: String,
    pub key1_private_path: String,
    pub identity_name: String,
    pub did: String,
    pub initial_jwt: String,
    pub bearer_seedings: Vec<ConnectSessionBearerSeed>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectSessionSimulation {
    pub actions: Vec<ConnectSessionAction>,
    pub auth_plan: Option<ConnectSessionAuthPlan>,
    pub returned_record_jwt: Option<String>,
    pub error: Option<String>,
}

pub fn simulate_connect_session(inputs: ConnectSessionInputs) -> ConnectSessionSimulation {
    let mut actions = vec![ConnectSessionAction::LoadIdentity {
        identity_name: inputs.identity_name.clone(),
    }];
    let record = match inputs.load {
        ConnectSessionLoadOutcome::Loaded(record) => record,
        ConnectSessionLoadOutcome::Error(error) => {
            return ConnectSessionSimulation {
                actions,
                auth_plan: None,
                returned_record_jwt: None,
                error: Some(error),
            };
        }
    };

    actions.push(ConnectSessionAction::EvaluateUserState {
        identity_name: record.identity_name.clone(),
    });
    if let Some(error) = user_registration_error(&record) {
        return ConnectSessionSimulation {
            actions,
            auth_plan: None,
            returned_record_jwt: None,
            error: Some(error),
        };
    }

    actions.push(ConnectSessionAction::PathsForIdentity {
        identity_name: inputs.identity_name.clone(),
    });
    let paths = match inputs.paths {
        Some(ConnectSessionPathsOutcome::Loaded(paths)) => paths,
        Some(ConnectSessionPathsOutcome::Error(error)) => {
            return ConnectSessionSimulation {
                actions,
                auth_plan: None,
                returned_record_jwt: None,
                error: Some(error),
            };
        }
        None => {
            return ConnectSessionSimulation {
                actions,
                auth_plan: None,
                returned_record_jwt: None,
                error: Some("identity paths outcome is required".to_string()),
            };
        }
    };

    actions.push(ConnectSessionAction::NewAuthSession {
        identity_name: record.identity_name.clone(),
    });
    let auth_plan = connect_session_auth_plan(&inputs.service_base_url, &record, &paths);
    for seed in &auth_plan.bearer_seedings {
        actions.push(ConnectSessionAction::SeedBearer {
            scope: seed.scope.clone(),
            token: seed.token.clone(),
        });
    }

    actions.push(ConnectSessionAction::NewWsClient);
    match inputs.ws_client {
        Some(ConnectSessionClientOutcome::Constructed) => {}
        Some(ConnectSessionClientOutcome::Error(error)) => {
            return ConnectSessionSimulation {
                actions,
                auth_plan: Some(auth_plan),
                returned_record_jwt: None,
                error: Some(error),
            };
        }
        None => {
            return ConnectSessionSimulation {
                actions,
                auth_plan: Some(auth_plan),
                returned_record_jwt: None,
                error: Some("websocket client construction outcome is required".to_string()),
            };
        }
    }

    actions.push(ConnectSessionAction::ConnectWithTimeout {
        timeout: LISTENER_CONNECT_TIMEOUT,
    });
    match inputs.connect {
        Some(ConnectSessionConnectOutcome::Connected { current_jwt }) => {
            actions.push(ConnectSessionAction::UpdateRecordJwt {
                jwt_token: current_jwt.clone(),
            });
            actions.push(ConnectSessionAction::ReturnConnected);
            ConnectSessionSimulation {
                actions,
                auth_plan: Some(auth_plan),
                returned_record_jwt: Some(current_jwt),
                error: None,
            }
        }
        Some(ConnectSessionConnectOutcome::Error(error)) => {
            actions.push(ConnectSessionAction::CloseClient);
            ConnectSessionSimulation {
                actions,
                auth_plan: Some(auth_plan),
                returned_record_jwt: None,
                error: Some(error),
            }
        }
        None => ConnectSessionSimulation {
            actions,
            auth_plan: Some(auth_plan),
            returned_record_jwt: None,
            error: Some("websocket connect outcome is required".to_string()),
        },
    }
}

pub fn connect_session_auth_plan(
    service_base_url: &str,
    record: &ConnectSessionRecord,
    paths: &ConnectSessionPaths,
) -> ConnectSessionAuthPlan {
    ConnectSessionAuthPlan {
        did_document_path: paths.did_document_path.clone(),
        key1_private_path: paths.key1_private_path.clone(),
        identity_name: record.identity_name.clone(),
        did: record.did.clone(),
        initial_jwt: record.jwt_token.clone(),
        bearer_seedings: connect_session_bearer_seedings(service_base_url, &record.jwt_token),
    }
}

pub fn connect_session_bearer_seedings(
    service_base_url: &str,
    jwt_token: &str,
) -> Vec<ConnectSessionBearerSeed> {
    if jwt_token.trim().is_empty() {
        return Vec::new();
    }
    let Ok(plan) = im_core::realtime::realtime_client_construction_plan(service_base_url) else {
        return Vec::new();
    };
    plan.remembered_scope_inputs
        .into_iter()
        .map(|scope| ConnectSessionBearerSeed {
            scope,
            token: jwt_token.to_string(),
        })
        .collect()
}

fn user_registration_error(record: &ConnectSessionRecord) -> Option<String> {
    let state = evaluate_user_state(&record.user_id, &record.handle);
    if state.ready_for_messaging {
        return None;
    }
    let identity_name = if record.identity_name.trim().is_empty() {
        "active identity"
    } else {
        record.identity_name.trim()
    };
    let missing = if state.missing.is_empty() {
        "user registration metadata".to_string()
    } else {
        state.missing.join(", ")
    };
    Some(format!(
        "registered handle user is required: identity {} is {} and missing {}; complete user setup with `awiki-cli id register --handle <handle> ...` or recover an existing handle first",
        identity_name, state.registration_state, missing
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConnectSessionUserState {
    registration_state: &'static str,
    ready_for_messaging: bool,
    missing: Vec<String>,
}

fn evaluate_user_state(user_id: &str, handle: &str) -> ConnectSessionUserState {
    let mut missing = Vec::new();
    if user_id.trim().is_empty() {
        missing.push("registration".to_string());
    }
    if handle.trim().is_empty() {
        missing.push("handle".to_string());
    }
    let registration_state = match missing.len() {
        0 => "registered_user",
        1 => "partial_user",
        _ => "local_identity",
    };
    ConnectSessionUserState {
        registration_state,
        ready_for_messaging: missing.is_empty(),
        missing,
    }
}
