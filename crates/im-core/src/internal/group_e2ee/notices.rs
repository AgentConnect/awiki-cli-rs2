use std::future::Future;

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GroupE2eeNoticeNotificationProjection {
    pub(crate) notification: Option<Value>,
    pub(crate) warnings: Vec<String>,
    pub(crate) processed: bool,
}

pub(crate) fn maybe_process_group_e2ee_notice_notification_for_client(
    client: &crate::core::ImClient,
    notification: Value,
) -> GroupE2eeNoticeNotificationProjection {
    maybe_process_group_e2ee_notice_notification(notification, |group| {
        client.secure().group(group).repair()
    })
}

pub(crate) async fn maybe_process_group_e2ee_notice_notification_for_client_async(
    client: &crate::core::ImClient,
    notification: Value,
) -> GroupE2eeNoticeNotificationProjection {
    maybe_process_group_e2ee_notice_notification_async(notification, |group| async move {
        client.secure().group(group).repair_async().await
    })
    .await
}

pub(crate) fn maybe_process_group_e2ee_notice_notification(
    notification: Value,
    mut process_group: impl FnMut(
        crate::ids::GroupRef,
    ) -> crate::ImResult<crate::secure::GroupSecureRepairResult>,
) -> GroupE2eeNoticeNotificationProjection {
    let group = match classify_group_e2ee_notice_notification(notification) {
        GroupE2eeNoticeNotificationMatch::NotNotice(notification) => {
            return GroupE2eeNoticeNotificationProjection {
                notification: Some(notification),
                warnings: Vec::new(),
                processed: false,
            };
        }
        GroupE2eeNoticeNotificationMatch::Drop(warnings) => {
            return GroupE2eeNoticeNotificationProjection {
                notification: None,
                warnings,
                processed: false,
            };
        }
        GroupE2eeNoticeNotificationMatch::Process(group) => group,
    };

    group_e2ee_notice_projection_from_repair_result(process_group(group))
}

pub(crate) async fn maybe_process_group_e2ee_notice_notification_async<F, Fut>(
    notification: Value,
    mut process_group: F,
) -> GroupE2eeNoticeNotificationProjection
where
    F: FnMut(crate::ids::GroupRef) -> Fut,
    Fut: Future<Output = crate::ImResult<crate::secure::GroupSecureRepairResult>>,
{
    let group = match classify_group_e2ee_notice_notification(notification) {
        GroupE2eeNoticeNotificationMatch::NotNotice(notification) => {
            return GroupE2eeNoticeNotificationProjection {
                notification: Some(notification),
                warnings: Vec::new(),
                processed: false,
            };
        }
        GroupE2eeNoticeNotificationMatch::Drop(warnings) => {
            return GroupE2eeNoticeNotificationProjection {
                notification: None,
                warnings,
                processed: false,
            };
        }
        GroupE2eeNoticeNotificationMatch::Process(group) => group,
    };

    group_e2ee_notice_projection_from_repair_result(process_group(group).await)
}

enum GroupE2eeNoticeNotificationMatch {
    NotNotice(Value),
    Drop(Vec<String>),
    Process(crate::ids::GroupRef),
}

fn classify_group_e2ee_notice_notification(
    notification: Value,
) -> GroupE2eeNoticeNotificationMatch {
    if !is_group_e2ee_notice_notification(&notification) {
        return GroupE2eeNoticeNotificationMatch::NotNotice(notification);
    }

    let Some(group_did) = group_did_from_notice_notification(&notification) else {
        return GroupE2eeNoticeNotificationMatch::Drop(vec![
            "group E2EE notice notification was dropped because group_did is missing".to_owned(),
        ]);
    };
    let group = match crate::ids::GroupRef::parse(&group_did) {
        Ok(group) => group,
        Err(err) => {
            return GroupE2eeNoticeNotificationMatch::Drop(vec![format!(
                "group E2EE notice notification was dropped because group_did is invalid: {err}"
            )]);
        }
    };

    GroupE2eeNoticeNotificationMatch::Process(group)
}

fn group_e2ee_notice_projection_from_repair_result(
    result: crate::ImResult<crate::secure::GroupSecureRepairResult>,
) -> GroupE2eeNoticeNotificationProjection {
    match result {
        Ok(result) => {
            let mut warnings = result.warnings;
            if !result.repaired {
                warnings.push(
                    "group E2EE notice notification was received, but no local MLS repair work was needed"
                        .to_owned(),
                );
            }
            GroupE2eeNoticeNotificationProjection {
                notification: None,
                warnings: compact_warnings(warnings),
                processed: true,
            }
        }
        Err(err) => GroupE2eeNoticeNotificationProjection {
            notification: None,
            warnings: vec![format!(
                "group E2EE notice notification processing failed: {err}"
            )],
            processed: false,
        },
    }
}

fn is_group_e2ee_notice_notification(notification: &Value) -> bool {
    notification.get("method").and_then(Value::as_str) == Some("group.e2ee.notice")
}

fn group_did_from_notice_notification(notification: &Value) -> Option<String> {
    [
        notification.pointer("/params/body/group_did"),
        notification.pointer("/params/body/group_state_ref/group_did"),
        notification.pointer("/params/body/e2ee_notice/group_did"),
        notification.pointer("/params/body/e2ee_notice/group_state_ref/group_did"),
    ]
    .into_iter()
    .flatten()
    .filter_map(string_from_value)
    .map(|value| value.trim().to_owned())
    .find(|value| !value.is_empty())
}

fn string_from_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn compact_warnings(warnings: Vec<String>) -> Vec<String> {
    let mut compact = Vec::new();
    for warning in warnings {
        if !warning.trim().is_empty() && !compact.contains(&warning) {
            compact.push(warning);
        }
    }
    compact
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn non_group_e2ee_notice_notification_is_kept() {
        let input = json!({
            "method": "group.incoming",
            "params": {"body": {"group_did": "did:example:group"}}
        });
        let projection = maybe_process_group_e2ee_notice_notification(input.clone(), |_| {
            unreachable!("non-notice notifications must not trigger MLS repair")
        });

        assert_eq!(projection.notification, Some(input));
        assert!(projection.warnings.is_empty());
        assert!(!projection.processed);
    }

    #[test]
    fn group_e2ee_notice_notification_triggers_repair_and_drops_raw_notice() {
        let projection = maybe_process_group_e2ee_notice_notification(
            group_notice_notification("did:example:group", "SECRET-WELCOME"),
            |group| {
                assert_eq!(group.as_str(), "did:example:group");
                Ok(crate::secure::GroupSecureRepairResult {
                    group,
                    state: crate::secure::GroupSecureState::Ready,
                    repaired: true,
                    added_devices: 0,
                    removed_devices: 0,
                    remaining_devices: 0,
                    problem: None,
                    warnings: vec!["repair warning".to_owned()],
                })
            },
        );

        assert!(projection.notification.is_none());
        assert_eq!(projection.warnings, vec!["repair warning".to_owned()]);
        assert!(projection.processed);
        assert!(!format!("{projection:?}").contains("SECRET-WELCOME"));
    }

    #[tokio::test]
    async fn group_e2ee_notice_notification_async_triggers_repair_and_drops_raw_notice() {
        let projection = maybe_process_group_e2ee_notice_notification_async(
            group_notice_notification("did:example:group", "SECRET-WELCOME"),
            |group| async move {
                assert_eq!(group.as_str(), "did:example:group");
                Ok(crate::secure::GroupSecureRepairResult {
                    group,
                    state: crate::secure::GroupSecureState::Ready,
                    repaired: true,
                    added_devices: 0,
                    removed_devices: 0,
                    remaining_devices: 0,
                    problem: None,
                    warnings: vec!["async repair warning".to_owned()],
                })
            },
        )
        .await;

        assert!(projection.notification.is_none());
        assert_eq!(projection.warnings, vec!["async repair warning".to_owned()]);
        assert!(projection.processed);
        assert!(!format!("{projection:?}").contains("SECRET-WELCOME"));
    }

    #[test]
    fn malformed_group_e2ee_notice_notification_is_dropped_without_raw_notice() {
        let projection = maybe_process_group_e2ee_notice_notification(
            json!({
                "method": "group.e2ee.notice",
                "params": {
                    "body": {
                        "welcome_b64u": "SECRET-WELCOME"
                    }
                }
            }),
            |_| unreachable!("malformed notice notifications must not trigger MLS repair"),
        );

        assert!(projection.notification.is_none());
        assert_eq!(projection.warnings.len(), 1);
        assert!(!format!("{projection:?}").contains("SECRET-WELCOME"));
    }

    fn group_notice_notification(group_did: &str, welcome_b64u: &str) -> Value {
        json!({
            "method": "group.e2ee.notice",
            "params": {
                "meta": {
                    "profile": anp::group_e2ee::PROFILE,
                    "content_type": "application/json"
                },
                "body": {
                    "notice_id": "notice-1",
                    "notice_type": "welcome-delivery",
                    "group_did": group_did,
                    "welcome_b64u": welcome_b64u,
                    "ratchet_tree_b64u": "TREE"
                }
            }
        })
    }
}
