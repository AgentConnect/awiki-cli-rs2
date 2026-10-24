//! Restart-safe high-level continuation; the SDK WAL still owns each MLS commit.
use serde_json::{json, Value};

pub(crate) fn record(
    client: &crate::core::ImClient,
    key: &str,
    group: &str,
    request: Option<Value>,
    ready: bool,
) -> crate::ImResult<crate::groups::GroupOperationStatus> {
    let previous = super::journal::load(client, key)?;
    let mut status = crate::groups::GroupOperationStatus {
        operation_id: key.to_owned(),
        group_did: group.to_owned(),
        phase: if ready {
            "completed"
        } else if group.is_empty() {
            "submitted"
        } else {
            "preparing"
        }
        .to_owned(),
        business_committed: !group.is_empty(),
        encryption_ready: ready,
        reason: (!ready).then(|| "encryption_preparation_pending".to_owned()),
        next_action: if ready { "none" } else { "wait" }.to_owned(),
    };
    if !ready && request.is_none() {
        if let Some(old) = previous
            .as_ref()
            .filter(|v| v["group_did"] == group && v["phase"] == "retry")
        {
            status.phase = "retry".to_owned();
            status.reason = old["status"]["reason"].as_str().map(str::to_owned);
            status.next_action = "retry".to_owned();
        }
    }
    super::journal::save(
        client,
        key,
        json!({
            "phase": status.phase, "group_did": group,
            "request": request.or_else(|| previous.as_ref().and_then(|v| v.get("request")).cloned()),
            "attempts": previous.as_ref().and_then(|v| v["attempts"].as_u64()).unwrap_or(0),
            "next_attempt_at": if ready { 0 } else { previous.as_ref().and_then(|v| v["next_attempt_at"].as_i64()).unwrap_or(0) }, "status": status,
        }),
    )?;
    Ok(status)
}

pub(crate) async fn maintain(client: &crate::core::ImClient) {
    // Plain creates also retain an uncertain P4 operation; secure work checks its own gate.
    let lock = client
        .core_inner()
        .group_lifecycle_lock(client.current_identity().id.as_str(), "group-recovery");
    let _guard = lock.lock().await;
    let Ok(pending) = super::journal::pending(client) else {
        return;
    };
    for (key, mut value) in pending
        .into_iter()
        .filter(|(_, value)| {
            value["next_attempt_at"].as_i64().unwrap_or(0) <= chrono::Utc::now().timestamp()
        })
        .take(8)
    {
        let now = chrono::Utc::now().timestamp();
        if value["next_attempt_at"].as_i64().unwrap_or(0) > now {
            continue;
        }
        let attempts = value["attempts"].as_u64().unwrap_or(0).saturating_add(1);
        // Charge the attempt before remote IO so a timeout/restart retains backoff.
        value["attempts"] = json!(attempts);
        value["next_attempt_at"] = json!(now + (5i64 << attempts.min(6)).min(300));
        if super::journal::save(client, &key, value.clone()).is_err() {
            continue;
        }
        let group = value["group_did"].as_str().unwrap_or("");
        if group.is_empty() {
            if value["request"].get("member").is_some() {
                if let Ok(request) = serde_json::from_value::<
                    crate::groups::GroupMemberMutationRequest,
                >(value["request"].clone())
                {
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_secs(15),
                        Box::pin(client.groups().add_member_async(request)),
                    )
                    .await;
                }
            } else if let Ok(request) = serde_json::from_value::<crate::groups::GroupCreateRequest>(
                value["request"].clone(),
            ) {
                let _ = tokio::time::timeout(
                    std::time::Duration::from_secs(15),
                    Box::pin(client.groups().create_async(request)),
                )
                .await;
            }
        } else if let Ok(group) = crate::ids::GroupRef::parse(group) {
            let secure = client.secure();
            let service = secure.group(group);
            let repaired =
                tokio::time::timeout(std::time::Duration::from_secs(15), service.repair_async())
                    .await;
            if matches!(&repaired, Ok(Ok(_))) {
                if let Ok(status) = service.status_async().await {
                    if status.can_send_secure {
                        let _ = record(client, &key, status.group.as_str(), None, true);
                        if let Some(member) = value["request"]["member"].as_str() {
                            let _ = super::journal::save(
                                client,
                                &format!("member-wire:{}:{member}", status.group.as_str()),
                                Value::Null,
                            );
                        }
                    }
                }
            } else if let Ok(Err(error)) = repaired {
                let _ = failed(client, &key, &error);
            }
        }
    }
}

/// Persist only a stable classification, never remote diagnostics or protocol material.
pub(crate) fn failed(
    client: &crate::core::ImClient,
    key: &str,
    error: &crate::ImError,
) -> crate::ImResult<()> {
    let Some(mut value) = super::journal::load(client, key)? else {
        return Ok(());
    };
    let reason = match error {
        crate::ImError::PermissionDenied => "original_owner_device_required",
        crate::ImError::Service {
            code: Some(code), ..
        } if code == "group.member_encryption_not_ready" || code == "anp.target_not_found" => {
            "member_encryption_not_ready"
        }
        _ => "encryption_preparation_failed",
    };
    value["phase"] = json!("retry");
    value["status"]["phase"] = json!("retry");
    value["status"]["reason"] = json!(reason);
    value["status"]["next_action"] = json!("retry");
    super::journal::save(client, key, value)
}

pub(crate) fn problem_for_group(
    client: &crate::core::ImClient,
    group: &str,
) -> crate::ImResult<Option<crate::secure::SecureProblem>> {
    for (_, value) in super::journal::pending(client)? {
        if value["group_did"] != group || value["phase"] != "retry" {
            continue;
        }
        let reason = value["status"]["reason"].as_str().unwrap_or("");
        let (message, retryable) = match reason {
            "original_owner_device_required" => ("请在原群主设备上完成加密准备", false),
            "member_encryption_not_ready" => ("等待成员完成加密准备", true),
            _ => ("加密准备未完成，请检查连接后重试", true),
        };
        return Ok(Some(crate::secure::SecureProblem {
            code: crate::secure::SecureProblemCode::GroupStateUnavailable,
            message: message.to_owned(),
            retryable,
        }));
    }
    Ok(None)
}
