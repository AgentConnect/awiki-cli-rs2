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
    let mut value = previous.clone().unwrap_or_else(|| json!({}));
    if !value.is_object() {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: "invalid group recovery progress".to_owned(),
        });
    }
    if !ready && request.is_none() {
        // Hints have no authoritative version and may be repeated. Preserve
        // backoff, but fence a repair already in flight from completing this
        // newly observed signal without another pass.
        let generation = value["signal_generation"]
            .as_u64()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                detail: "group recovery signal generation exhausted".to_owned(),
            })?;
        value["signal_generation"] = json!(generation);
    }
    let progress = json!({
        "phase": status.phase, "group_did": group,
        "request": request.or_else(|| previous.as_ref().and_then(|v| v.get("request")).cloned()),
        "attempts": previous.as_ref().and_then(|v| v["attempts"].as_u64()).unwrap_or(0),
        "next_attempt_at": if ready { 0 } else { previous.as_ref().and_then(|v| v["next_attempt_at"].as_i64()).unwrap_or(0) }, "status": status,
    });
    let fields = value
        .as_object_mut()
        .ok_or_else(|| crate::ImError::LocalStateUnavailable {
            detail: "invalid group recovery progress".to_owned(),
        })?;
    fields.extend(progress.as_object().expect("progress object").clone());
    if let Some(previous) = previous {
        if !super::journal::replace_if_unchanged(client, key, &previous, value)? {
            return Err(crate::ImError::CursorStale);
        }
    } else {
        // Do not overwrite a state-change task inserted after the initial read.
        let stored = super::journal::freeze(client, key, key, value.clone())?;
        if stored != value {
            return Err(crate::ImError::CursorStale);
        }
    }
    Ok(status)
}

/// Complete exactly the task observed before repair. Newer state changes remain
/// pending even if the preceding repair happened to observe a sendable epoch.
pub(crate) fn complete_observed(
    client: &crate::core::ImClient,
    key: &str,
    observed: &Value,
) -> crate::ImResult<bool> {
    let mut completed = observed.clone();
    completed["phase"] = json!("completed");
    completed["next_attempt_at"] = json!(0);
    completed["status"]["phase"] = json!("completed");
    completed["status"]["encryption_ready"] = json!(true);
    completed["status"]["reason"] = Value::Null;
    completed["status"]["next_action"] = json!("none");
    super::journal::replace_if_unchanged(client, key, observed, completed)
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
        let observed = value.clone();
        value["attempts"] = json!(attempts);
        value["next_attempt_at"] = json!(now + (5i64 << attempts.min(6)).min(300));
        if !matches!(
            super::journal::replace_if_unchanged(client, &key, &observed, value.clone()),
            Ok(true)
        ) {
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
            let repaired = tokio::time::timeout(std::time::Duration::from_secs(15), async {
                if value
                    .get("last_change_id")
                    .and_then(Value::as_str)
                    .is_some()
                {
                    service.prepare_observed_async().await.map(|_| ())
                } else {
                    service.repair_async().await.map(|_| ())
                }
            })
            .await;
            // repair_async completes only the tasks it observed before IO and
            // publishes its validated state. Do not repeat status/roster reads.
            if let Ok(Err(error)) = repaired {
                let _ = fail_observed(client, &key, &value, &error);
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
    let Some(value) = super::journal::load(client, key)? else {
        return Ok(());
    };
    fail_observed(client, key, &value, error).map(|_| ())
}

fn fail_observed(
    client: &crate::core::ImClient,
    key: &str,
    observed: &Value,
    error: &crate::ImError,
) -> crate::ImResult<bool> {
    let mut value = observed.clone();
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
    super::journal::replace_if_unchanged(client, key, observed, value)
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
