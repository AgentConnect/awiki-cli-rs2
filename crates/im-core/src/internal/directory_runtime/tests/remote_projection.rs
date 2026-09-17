use super::*;

fn absent() -> crate::ImResult<Value> {
    Err(crate::ImError::Service {
        status_code: Some(200),
        code: Some("-32002".into()),
        message: "Handle 不存在".into(),
        data: None,
    })
}

#[tokio::test]
async fn remote_sender_projection_uses_verified_full_handle_after_reverse_lookup_absence() {
    let did = "did:wba:rwiki.cn:user:alice:e1_root";
    let raw = json!({"did":did,"full_handle":"alice.rwiki.cn","domain":"rwiki.cn","user_id":"authority-subject","status":"active"});
    let mut transport = RecordingTransport {
        calls: vec![],
        results: vec![absent(), Ok(raw.clone())],
    };
    assert_eq!(
        lookup_projection_binding_async(&mut transport, did, "awiki.info")
            .await
            .unwrap(),
        raw
    );
    assert_eq!(transport.calls.len(), 2);
    assert_eq!(transport.calls[0].2, json!({"did":did}));
    assert_eq!(transport.calls[1].2, json!({"handle":"alice.rwiki.cn"}));
}

#[tokio::test]
async fn remote_projection_rejects_rebound_or_wrong_handle_and_never_retries_auth_as_discovery() {
    let did = "did:wba:rwiki.cn:user:alice:e1_root";
    for raw in [
        json!({"did":"did:wba:rwiki.cn:user:alice:e1_other","full_handle":"alice.rwiki.cn"}),
        json!({"did":did,"full_handle":"other.rwiki.cn"}),
    ] {
        let mut transport = RecordingTransport {
            calls: vec![],
            results: vec![absent(), Ok(raw)],
        };
        assert!(matches!(
            lookup_projection_binding_async(&mut transport, did, "awiki.info").await,
            Err(crate::ImError::IdentityBindingConflict { .. })
        ));
    }
    let mut transport = RecordingTransport {
        calls: vec![],
        results: vec![Err(crate::ImError::PermissionDenied)],
    };
    assert!(matches!(
        lookup_projection_binding_async(&mut transport, did, "awiki.info").await,
        Err(crate::ImError::PermissionDenied)
    ));
    assert_eq!(transport.calls.len(), 1);
    let mut local = RecordingTransport {
        calls: vec![],
        results: vec![absent()],
    };
    assert!(lookup_projection_binding_async(&mut local, did, "rwiki.cn")
        .await
        .is_err());
    assert_eq!(local.calls.len(), 1);
}
