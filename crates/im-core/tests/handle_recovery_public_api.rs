use awiki_im_core::identity::{
    HandleRecoveryActivateRequest, HandleRecoveryErrorCode, HandleRecoveryOtpRequest,
    HandleRecoveryPhase, HandleRecoveryPrepareRequest,
};
use std::io::{Read as _, Write as _};

#[test]
fn recovery_secret_inputs_are_write_only_in_debug_output() {
    let otp = HandleRecoveryOtpRequest {
        identity: Some(awiki_im_core::identity::IdentitySelector::Default),
        full_handle: "alice.awiki.info".to_owned(),
        phone: "+8613800000000".to_owned(),
    };
    let prepare = HandleRecoveryPrepareRequest {
        operation_id: "recover-001".to_owned(),
        phone: "+8613800000000".to_owned(),
        code: "123456".to_owned(),
    };
    assert!(!format!("{otp:?}").contains("13800000000"));
    assert!(!format!("{prepare:?}").contains("123456"));
    assert!(!format!("{prepare:?}").contains("13800000000"));
}

#[test]
fn recovery_facade_uses_the_frozen_phase_and_error_vocabulary() {
    assert_eq!(
        serde_json::to_string(&HandleRecoveryPhase::IdentityTransitionPending).unwrap(),
        "\"identity_transition_pending\""
    );
    assert_eq!(
        HandleRecoveryErrorCode::OutcomeUnknown.as_str(),
        "outcome_unknown"
    );
    assert_eq!(
        HandleRecoveryErrorCode::LocalMigrationUnsupported.as_str(),
        "local_migration_unsupported"
    );
}

#[test]
fn recovery_v4_error_retryability_is_a_closed_table() {
    let cases = [
        (HandleRecoveryErrorCode::FactorRetryRequired, true),
        (HandleRecoveryErrorCode::ResultAbsent, true),
        (HandleRecoveryErrorCode::OutcomeUnknown, true),
        (HandleRecoveryErrorCode::LocalKeyUnavailable, false),
        (HandleRecoveryErrorCode::LocalTransitionPending, true),
        (HandleRecoveryErrorCode::LocalMigrationUnsupported, false),
        (HandleRecoveryErrorCode::UnknownEpoch, false),
    ];
    for (code, retryable) in cases {
        assert_eq!(code.retryable(), retryable, "{}", code.as_str());
    }
}

#[test]
fn schema_35_without_recovery_tables_upgrades_through_the_compat_boundary() {
    let db = rusqlite::Connection::open_in_memory().unwrap();
    awiki_im_core::compat::local_state::ensure_schema(&db).unwrap();
    db.execute_batch(
        r#"
DROP TABLE handle_recovery_operations_v4;
DROP TABLE identity_transition_pending;
PRAGMA user_version=35;
"#,
    )
    .unwrap();

    awiki_im_core::compat::local_state::ensure_schema(&db).unwrap();

    assert_eq!(
        awiki_im_core::compat::local_state::current_schema_version(&db).unwrap(),
        awiki_im_core::compat::local_state::SCHEMA_VERSION,
    );
    for table in [
        "identity_transition_pending",
        "handle_recovery_operations_v4",
    ] {
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1,
        );
    }
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('identity_transition_pending') WHERE name='metadata_json'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1,
    );
}

#[test]
fn activation_requires_an_explicit_user_presence_field() {
    let request = HandleRecoveryActivateRequest {
        operation_id: "recovery-public-ref".to_owned(),
        user_presence_confirmed: false,
    };
    assert!(!request.user_presence_confirmed);
}

#[tokio::test]
async fn recovery_execution_gate_defaults_off() {
    let temporary = tempfile::tempdir().unwrap();
    let core = awiki_im_core::ImCore::open(
        awiki_im_core::ImCoreConfig::new(
            awiki_im_core::ServiceEndpoint::parse("https://example.invalid").unwrap(),
            "example.invalid",
        )
        .unwrap(),
        awiki_im_core::ImCorePaths {
            identities: awiki_im_core::IdentityRegistryPaths {
                identity_root_dir: temporary.path().join("identities"),
                registry_path: temporary.path().join("identities/registry.json"),
                default_identity_path: Some(temporary.path().join("identities/default")),
            },
            local_state: awiki_im_core::LocalStatePaths {
                sqlite_path: temporary.path().join("local/im.sqlite"),
            },
            runtime: awiki_im_core::RuntimePaths {
                cache_dir: temporary.path().join("cache"),
                temp_dir: temporary.path().join("tmp"),
            },
        },
    )
    .await
    .unwrap();
    let error = core
        .handle_recovery()
        .request_handle_recovery_otp(HandleRecoveryOtpRequest {
            identity: None,
            full_handle: "alice.example.invalid".to_owned(),
            phone: "+8613800000000".to_owned(),
        })
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        awiki_im_core::ImError::UnsupportedCapability { capability }
            if capability == "handle-recovery-v4"
    ));
}

#[tokio::test]
async fn recovery_publishes_vault_record_before_sending_otp() {
    let temporary = tempfile::tempdir().unwrap();
    let paths = awiki_im_core::ImCorePaths {
        identities: awiki_im_core::IdentityRegistryPaths {
            identity_root_dir: temporary.path().join("identities"),
            registry_path: temporary.path().join("identities/registry.json"),
            default_identity_path: Some(temporary.path().join("identities/default")),
        },
        local_state: awiki_im_core::LocalStatePaths {
            sqlite_path: temporary.path().join("local/im.sqlite"),
        },
        runtime: awiki_im_core::RuntimePaths {
            cache_dir: temporary.path().join("cache"),
            temp_dir: temporary.path().join("tmp"),
        },
    };
    let vault_dir = temporary.path().join("identity-vault");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server_vault_dir = vault_dir.clone();
    let server_sqlite_path = paths.local_state.sqlite_path.clone();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_http_request(&mut stream);
        let body: serde_json::Value = serde_json::from_slice(request.body()).unwrap();
        let operation_id = body["params"]["operation_id"].as_str().unwrap();
        let records = std::fs::read_dir(server_vault_dir.join("records"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.path().extension().and_then(|value| value.to_str()) == Some("json")
            })
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 1, "the pre-OTP key journal must be durable");
        assert!(records[0].metadata().unwrap().len() > 0);
        let database = rusqlite::Connection::open(server_sqlite_path).unwrap();
        let lifecycle: String = database
            .query_row(
                "SELECT lifecycle_class FROM handle_recovery_operations_v4 WHERE operation_id = ?1",
                [operation_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(lifecycle, "pre_commit");
        write_json_response(
            &mut stream,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": body["id"],
                "result": {
                    "ok": true,
                    "retry_after_seconds": 60,
                    "retry_at": "2099-08-13T08:00:00Z"
                }
            }),
        );
        request
    });

    let core = awiki_im_core::ImCore::open_with_options(
        awiki_im_core::ImCoreConfig::new(
            awiki_im_core::ServiceEndpoint::parse(&endpoint).unwrap(),
            "awiki.test",
        )
        .unwrap(),
        paths.clone(),
        awiki_im_core::ImCoreOpenOptions::default()
            .with_multi_device_handle_recovery_enabled(true)
            .with_multi_device_audience("awiki-user-service")
            .with_identity_secret_vault(
                awiki_im_core::IdentitySecretStoragePolicy::VaultRequired,
                awiki_im_core::ImCoreSecretVaultOptions::new(
                    awiki_im_core::vault::DeviceVaultRootKey::from_bytes([81_u8; 32]),
                    &vault_dir,
                    "android-recovery-regression-workspace",
                    "android-recovery-regression-device",
                ),
            ),
    )
    .await
    .unwrap();

    let result = core
        .handle_recovery()
        .request_handle_recovery_otp(HandleRecoveryOtpRequest {
            identity: None,
            full_handle: "alice.awiki.test".to_owned(),
            phone: "+8613800000000".to_owned(),
        })
        .await
        .unwrap();

    assert!(result.accepted);
    assert_eq!(result.retry_after_seconds, 60);
    let request = server.join().unwrap();
    let body: serde_json::Value = serde_json::from_slice(request.body()).unwrap();
    assert_eq!(request.path, "/user-service/v1/handle/rpc");
    assert_eq!(body["method"], "send_otp");
    assert_eq!(body["params"]["operation_id"], result.operation_id);
}

struct CapturedHttp {
    path: String,
    bytes: Vec<u8>,
    body_offset: usize,
}

impl CapturedHttp {
    fn body(&self) -> &[u8] {
        &self.bytes[self.body_offset..]
    }
}

fn read_http_request(stream: &mut std::net::TcpStream) -> CapturedHttp {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    let body_offset = loop {
        let read = stream.read(&mut buffer).unwrap();
        assert!(read > 0, "request closed before headers");
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(offset) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break offset + 4;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..body_offset]);
    let path = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap()
        .to_owned();
    let content_length = headers
        .lines()
        .find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
        })
        .unwrap_or(0);
    while bytes.len() < body_offset + content_length {
        let read = stream.read(&mut buffer).unwrap();
        assert!(read > 0, "request closed before body");
        bytes.extend_from_slice(&buffer[..read]);
    }
    CapturedHttp {
        path,
        bytes,
        body_offset,
    }
}

fn write_json_response(stream: &mut std::net::TcpStream, value: &serde_json::Value) {
    let body = serde_json::to_vec(value).unwrap();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .unwrap();
    stream.write_all(&body).unwrap();
    stream.flush().unwrap();
}
