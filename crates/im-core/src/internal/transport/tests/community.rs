use super::*;

#[tokio::test]
async fn community_session_selects_before_first_operation_and_revalidates_cached_mode() {
    use crate::internal::auth::session::{
        AsyncSessionProvider, FileSessionProvider, SessionProvider,
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let root = tempfile::tempdir().unwrap();
    let core = host_backed_core(
        root.path(),
        &format!("http://{}", listener.local_addr().unwrap()),
    );
    let (client, bootstrap, _) = host_backed_client(&core);
    let fixture: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/testdata/message-sync/community-sync-v1.json"
    )))
    .unwrap();
    let mut caps = fixture["cases"]["C01"]["response"]["result"].clone();
    caps["service_did"] = json!("did:wba:awiki.test");
    let token = host_device_access_token(
        &bootstrap,
        HOST_ACCOUNT_ID,
        bootstrap.protocol_device_id.as_str(),
        &bootstrap.device_signing_key_id,
        1,
        &["device:manage", "device:read", "message:connect"],
        "session-discovery-token",
    );
    let server_token = token.clone();
    let server = std::thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        for (index, result) in [caps.clone(), json!({"supported_profiles":["awiki.message-sync.explicit-negotiation.v1","sync.snapshot_paging.v1"]}), caps].into_iter().enumerate() {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind()==std::io::ErrorKind::WouldBlock && Instant::now()<deadline => std::thread::sleep(Duration::from_millis(10)),
                    Err(_) => panic!("expected session discovery was not received"),
                }
            };
            let request = read_request_headers(&mut stream);
            assert!(request.starts_with("POST /im/rpc "));
            let body = serde_json::to_vec(&json!({"jsonrpc":"2.0","id":"req-1","result":result})).unwrap();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",body.len()).unwrap();
            if index==0 { write!(stream,"Authentication-Info: access_token={server_token}\r\n").unwrap(); }
            stream.write_all(b"\r\n").unwrap(); stream.write_all(&body).unwrap(); stream.flush().unwrap();
        }
    });
    assert_eq!(
        crate::internal::community_sync::cached_mode(&client).unwrap(),
        None
    );
    let provider = FileSessionProvider::new(&client);
    let session =
        AsyncSessionProvider::ensure_session(&provider, crate::auth::AuthScope::GroupMessaging)
            .await
            .unwrap();
    assert!(session.bearer_token.as_deref() == Some(token.as_str()));
    assert!(session.refreshed);
    assert!(
        AsyncSessionProvider::ensure_session(&provider, crate::auth::AuthScope::Messaging)
            .await
            .is_err()
    );
    assert_eq!(
        crate::internal::community_sync::cached_mode(&client).unwrap(),
        Some(crate::internal::community_sync::SyncServiceMode::Community)
    );
    let session =
        SessionProvider::ensure_session(&provider, crate::auth::AuthScope::GroupMessaging).unwrap();
    assert!(session.bearer_token.as_deref() == Some(token.as_str()));
    server.join().unwrap();
}

#[tokio::test]
async fn community_auth_renews_at_get_me_and_retries_business_once() {
    for asynchronous in [false, true] {
        if !asynchronous && !cfg!(feature = "blocking") {
            continue;
        }
        for missing in [false, true] {
            for denied_after_refresh in [false, true] {
                let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                let endpoint = format!("http://{}", listener.local_addr().unwrap());
                let root = tempfile::tempdir().unwrap();
                let core = host_backed_core(root.path(), &endpoint);
                let (client, bootstrap, _) = host_backed_client(&core);
                let fixture: Value = serde_json::from_str(include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/testdata/message-sync/community-sync-v1.json"
                )))
                .unwrap();
                let mut capabilities = fixture["cases"]["C01"]["response"]["result"].clone();
                capabilities["service_did"] = json!("did:wba:awiki.test");
                crate::internal::community_sync::confirm(&client, &capabilities)
                    .await
                    .unwrap();
                let refreshed = host_device_access_token(
                    &bootstrap,
                    HOST_ACCOUNT_ID,
                    bootstrap.protocol_device_id.as_str(),
                    &bootstrap.device_signing_key_id,
                    1,
                    &["device:manage", "device:read", "message:connect"],
                    "community-renewal",
                );
                let server_token = refreshed.clone();
                let server = std::thread::spawn(move || {
                    listener.set_nonblocking(true).unwrap();
                    let accept = || {
                        let deadline = Instant::now() + Duration::from_secs(10);
                        loop {
                            match listener.accept() {
                                Ok((stream, _)) => break stream,
                                Err(error)
                                    if error.kind() == std::io::ErrorKind::WouldBlock
                                        && Instant::now() < deadline =>
                                {
                                    std::thread::sleep(Duration::from_millis(10))
                                }
                                Err(_) => {
                                    panic!("expected Community auth request was not received")
                                }
                            }
                        }
                    };
                    if !missing {
                        let mut stream = accept();
                        let request = read_request_headers(&mut stream);
                        assert!(request.starts_with("POST /im/rpc "));
                        write_unauthorized(&mut stream);
                    }
                    let mut stream = accept();
                    let renewal = read_request_headers(&mut stream);
                    assert!(renewal.starts_with(&format!(
                        "POST {} ",
                        crate::internal::identity_wire::DID_AUTH_RPC_ENDPOINT
                    )));
                    assert!(!renewal
                        .to_ascii_lowercase()
                        .contains("authorization: bearer"));
                    assert!(renewal.to_ascii_lowercase().contains("signature-input:"));
                    write_rpc_success_with_body_token(&mut stream, &server_token);
                    drop(stream);
                    let mut stream = accept();
                    let request = read_request_headers(&mut stream);
                    assert!(request.starts_with("POST /im/rpc "));
                    assert!(request.to_ascii_lowercase().contains(
                        &format!("authorization: bearer {server_token}").to_ascii_lowercase()
                    ));
                    assert!(!request.to_ascii_lowercase().contains("signature-input:"));
                    if denied_after_refresh {
                        write_unauthorized(&mut stream);
                    } else {
                        write_rpc_success(&mut stream);
                    }
                });
                let mut transport = CoreHttpTransport::new(&client);
                if missing {
                    transport.deferred_auth_state_error = Some(DeferredAuthStateError::Missing);
                }
                let result = if asynchronous {
                    AsyncAuthenticatedRpcTransport::authenticated_rpc(
                        &mut transport,
                        "/im/rpc",
                        "sync.delta",
                        json!({}),
                    )
                    .await
                } else {
                    AuthenticatedRpcTransport::authenticated_rpc(
                        &mut transport,
                        "/im/rpc",
                        "sync.delta",
                        json!({}),
                    )
                };
                if !denied_after_refresh && result.is_err() {
                    panic!("Community auth failed (async={asynchronous}, missing={missing}): {result:?}");
                }
                server.join().unwrap();
                if denied_after_refresh {
                    assert!(matches!(
                        result,
                        Err(crate::ImError::Service {
                            status_code: Some(401),
                            ..
                        })
                    ));
                } else {
                    assert_eq!(result.unwrap(), json!({"ok":true}));
                }
                assert!(transport.last_auth_retry_consumed);
                assert!(transport.jwt_token.as_deref() == Some(refreshed.as_str()));
            }
        }
    }
}
