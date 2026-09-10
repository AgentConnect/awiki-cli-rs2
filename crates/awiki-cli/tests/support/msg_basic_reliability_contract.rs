// Uses the exact registered identity and isolated HOME from the inbox contract fixture.
#[test]
fn direct_retry_across_cli_processes_reuses_request_and_reports_outgoing_read() {
    for (label, identity_flags, lose_first_response) in [
        (
            "explicit",
            vec![
                "--client-message-id",
                "msg-basic-retry",
                "--idempotency-key",
                "op-basic-retry",
            ],
            false,
        ),
        (
            "message-id-only",
            vec!["--client-message-id", "msg-basic-retry"],
            false,
        ),
        (
            "operation-only-lost-response",
            vec!["--idempotency-key", "op-basic-retry"],
            true,
        ),
    ] {
        let workspace = TempDir::new(label).unwrap();
        let identity = register_exact_msg_identity(workspace.path());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        write_msg_ws_config(workspace.path(), &url);
        write_tenant_config(workspace.path(), "runtime:\n  mode: http\n");
        let server = thread::spawn(move || {
            let mut captured = vec![];
            let attempts = if lose_first_response { 3 } else { 2 };
            for index in 0..attempts {
                let mut stream = accept_with_timeout(&listener).expect("Direct send request");
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let request = read_http_request(&mut stream);
                let rpc: Value = serde_json::from_str(request_body(&request)).unwrap();
                assert_eq!(rpc["method"], "direct.send");
                let params = rpc["params"].clone();
                captured.push(params.clone());
                // Both attempts of the first CLI invocation lose their response.
                if lose_first_response && index < 2 {
                    continue;
                }
                write_http_response(&mut stream, &json!({"jsonrpc": "2.0", "id": rpc["id"], "result": {
                    "message_id": params["meta"]["message_id"], "operation_id": params["meta"]["operation_id"],
                    "target_did": params["meta"]["target"]["did"], "delivery_state": "accepted", "accepted_at": "2026-09-10T00:00:00Z"
                }}).to_string());
            }
            captured
        });
        let mut args = vec![
            "--identity",
            "alice",
            "msg",
            "send",
            "--to",
            identity.did.as_str(),
            "--text",
            "reliable direct retry",
        ];
        args.extend(identity_flags);
        let first = awiki_cmd(&args, workspace.path());
        if lose_first_response {
            assert!(!first.status.success());
        } else {
            assert_success(&first);
        }
        thread::sleep(Duration::from_millis(1100));
        let second = awiki_cmd(&args, workspace.path());
        assert_success(&second);
        let requests = server.join().unwrap();
        for retry in &requests[1..] {
            assert_eq!(requests[0]["meta"], retry["meta"], "{label}");
            assert_eq!(requests[0]["body"], retry["body"], "{label}");
        }
        let message_id = requests[0]["meta"]["message_id"].as_str().unwrap();
        let rows = query_rows(
            workspace.path(),
            "SELECT msg_id, direction, is_read FROM messages",
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["msg_id"], message_id);
        assert_eq!(rows[0]["direction"], 1);
        assert_eq!(rows[0]["is_read"], 1);

        let marked = awiki_cmd(
            &["--identity", "alice", "msg", "mark-read", message_id],
            workspace.path(),
        );
        assert_eq!(
            marked.status.code(),
            Some(2),
            "{}",
            String::from_utf8_lossy(&marked.stderr)
        );
        let error: Value = serde_json::from_slice(&marked.stderr).unwrap();
        assert_eq!(error["error"]["code"], "message_not_incoming");
        assert!(error["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("msg inbox"));

        let text_index = args.iter().position(|arg| *arg == "--text").unwrap() + 1;
        args[text_index] = "changed content";
        let changed = awiki_cmd(&args, workspace.path());
        assert_eq!(changed.status.code(), Some(2));
        let error: Value = serde_json::from_slice(&changed.stderr).unwrap();
        assert_eq!(error["error"]["code"], "message_retry_conflict");
    }
}
