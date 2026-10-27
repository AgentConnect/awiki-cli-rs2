use super::*;

fn capabilities() -> Map<String, Value> {
    json!({"schema_version":1,"workspace_binding":"workspace-a","methods":["local.send_message"]})
        .as_object()
        .unwrap()
        .clone()
}

#[test]
fn uncertain_ipc_repeats_exact_intent_and_never_chooses_standalone_send() {
    let mut requests = vec![];
    let result = invoke_with("workspace-a", "alice", "local.send_message",
        json!({"request":{"client_message_id":"message-1","delivery":{"idempotency_key":"op-1"},"text":"original"}}).as_object().unwrap().clone(),
        json!({"client_message_id":"message-1","idempotency_key":"op-1"}), |request| {
            requests.push(request);
            match requests.len() {
                1 => Ok(capabilities()),
                2 => Err(BridgeCallError::new("bridge_read", "response lost", "EOF").into()),
                3 => Ok(json!({"data":{"accepted":true},"summary":"sent","warnings":[]}).as_object().unwrap().clone()),
                _ => panic!("retry must be bounded"),
            }
        }).unwrap().unwrap();
    assert_eq!(result.data["accepted"], true);
    assert_eq!(requests[1], requests[2]);
    assert_eq!(requests[1].identity_name, "alice");
    assert_eq!(requests[1].params["workspace_binding"], "workspace-a");
}

#[test]
fn repeated_ipc_loss_returns_original_ids_instead_of_fallback() {
    let mut calls = 0;
    let recovery = json!({"client_message_id":"message-1","idempotency_key":"op-1"});
    let error = invoke_with(
        "workspace-a",
        "alice",
        "local.send_message",
        Map::new(),
        recovery.clone(),
        |_| {
            calls += 1;
            if calls == 1 {
                Ok(capabilities())
            } else {
                Err(BridgeCallError::new("bridge_read", "response lost", "EOF").into())
            }
        },
    )
    .unwrap_err();
    assert_eq!(calls, 3);
    assert_eq!(error.detail.details, recovery);
    assert!(error.detail.retryable);
}

#[test]
fn legacy_probe_and_wrong_workspace_never_submit_an_intent() {
    let mut calls = 0;
    let result = invoke_with(
        "workspace-a",
        "alice",
        "local.send_message",
        Map::new(),
        Value::Null,
        |_| {
            calls += 1;
            Err(anyhow::anyhow!("unsupported local listener bridge method"))
        },
    )
    .unwrap();
    assert!(result.is_none());
    assert_eq!(calls, 1);
    let mut calls = 0;
    let error = invoke_with(
        "workspace-b",
        "alice",
        "local.send_message",
        Map::new(),
        Value::Null,
        |_| {
            calls += 1;
            Ok(capabilities())
        },
    )
    .unwrap_err();
    assert_eq!(error.detail.code, "identity_binding_conflict");
    assert_eq!(calls, 1);
}

#[test]
fn remote_command_error_preserves_public_category_without_retry() {
    let mut calls = 0;
    let error = invoke_with(
        "workspace-a",
        "alice",
        "local.send_message",
        Map::new(),
        Value::Null,
        |_| {
            calls += 1;
            if calls == 1 {
                return Ok(capabilities());
            }
            Ok(json!({"command_error":{"exit_code":4,"error":{
                "code":"permission_denied","message":"not permitted","retryable":false
            }}})
            .as_object()
            .unwrap()
            .clone())
        },
    )
    .unwrap_err();
    assert_eq!(error.detail.code, "permission_denied");
    assert_eq!(error.exit_code, 4);
    assert_eq!(calls, 2);
}

#[test]
fn generated_send_ids_are_stable_and_explicit_ids_are_preserved() {
    let (mut message, mut operation) = (None, None);
    ids(&mut message, &mut operation).unwrap();
    let first = (message.clone(), operation.clone());
    ids(&mut message, &mut operation).unwrap();
    assert_eq!((message, operation), first);
    let mut message = Some(MessageId::parse("explicit-message").unwrap());
    let mut operation = Some("explicit-operation".to_owned());
    ids(&mut message, &mut operation).unwrap();
    assert_eq!(message.unwrap().as_str(), "explicit-message");
    assert_eq!(operation.unwrap(), "explicit-operation");
}
