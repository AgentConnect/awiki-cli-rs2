use super::*;

fn fixture() -> (V2PreparedRemoveSubmission, crate::ImError) {
    let submission: V2PreparedRemoveSubmission = V2PreparedRemoveSubmission {
        meta: serde_json::from_value(json!({
            "profile":"anp.group.e2ee.v2", "security_profile":"group-e2ee",
            "sender_did":"did:example:owner", "sender_device_id":"owner-device",
            "target":{"kind":"group","did":"did:example:group"}, "operation_id":"remove-1"
        }))
        .unwrap(),
        prepared: serde_json::from_value(json!({
            "pending_commit_id":"pending-1", "from_epoch":"2",
            "body":{"member_did":"did:example:member", "member_device_id":"removed-device",
                "group_state_ref":{"group_did":"did:example:group","group_state_version":"1"},
                "crypto_group_id_b64u":"Z3JvdXA", "epoch":"3", "commit_b64u":"Y29tbWl0"}
        }))
        .unwrap(),
    };
    let body = &submission.prepared.body;
    let proof = json!({
        "schema_version":1,"reason":"competing_commit_accepted","method":"group.e2ee.remove",
        "operation_id":"remove-1","sender_did":"did:example:owner","sender_device_id":"owner-device",
        "group_state_ref":body.group_state_ref,"crypto_group_id_b64u":body.crypto_group_id_b64u,
        "epoch":"3","member_did":"did:example:member","member_device_id":"removed-device",
        "commit_b64u_sha256":URL_SAFE_NO_PAD.encode(Sha256::digest(body.commit_b64u.as_bytes())),
    });
    (
        submission,
        crate::ImError::Service {
            status_code: Some(200),
            code: Some("anp.target_not_found".into()),
            message: "missing active leaf".into(),
            data: Some(json!({"rejected_membership_submission":proof})),
        },
    )
}

#[test]
fn group_remove_recovery_requires_exact_proof_and_never_uses_a_missing_target_alone() {
    let (submission, error) = fixture();
    assert!(proven_competing_remove(&error, &submission));
    let crate::ImError::Service {
        data: Some(data), ..
    } = &error
    else {
        unreachable!()
    };
    for key in data["rejected_membership_submission"]
        .as_object()
        .unwrap()
        .keys()
    {
        let mut changed = error.clone();
        if let crate::ImError::Service {
            data: Some(data), ..
        } = &mut changed
        {
            data["rejected_membership_submission"][key] = Value::Null;
        }
        assert!(
            !proven_competing_remove(&changed, &submission),
            "missing or mismatched {key}"
        );
    }
    for replacement in [
        None,
        Some(json!({})),
        Some(json!({"rejected_membership_submission":{"schema_version":2}})),
    ] {
        let mut changed = error.clone();
        if let crate::ImError::Service { data, .. } = &mut changed {
            *data = replacement;
        }
        assert!(!proven_competing_remove(&changed, &submission));
    }
    let mut changed = submission;
    changed.prepared.body.commit_b64u = "YW5vdGhlcg".into();
    assert!(!proven_competing_remove(&error, &changed));
    assert!(!proven_competing_remove(
        &crate::ImError::CursorStale,
        &changed
    ));
}
