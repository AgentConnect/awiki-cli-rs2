//! Public creation discovery uses the real HTTP transport, without an identity.
use awiki_im_core::identity::DidMethod;
use awiki_im_core::prelude::*;
use std::io::{Read, Write};
use std::net::TcpListener;

#[tokio::test]
async fn creation_discovery_uses_unsigned_get_and_the_explicit_create_gate() {
    for (status, body, expected) in [
        (
            200,
            r#"{"identity":{"did_methods":[{"id":"wba","create":true},{"id":"web","create":true}]}}"#,
            vec![DidMethod::Wba, DidMethod::Web],
        ),
        (
            200,
            r#"{"identity":{"did_methods":[{"id":"wba","create":true},{"id":"web","create":false}]}}"#,
            vec![DidMethod::Wba],
        ),
        (404, r#"{"detail":"Not Found"}"#, vec![DidMethod::Wba]),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0_u8; 1024];
            while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                let read = stream.read(&mut buffer).unwrap();
                assert!(read > 0);
                bytes.extend_from_slice(&buffer[..read]);
            }
            let request = String::from_utf8(bytes).unwrap();
            let valid = request.starts_with("GET /user-service/v1/server-info HTTP/")
                && !request.to_lowercase().contains("authorization:");
            let reply_status = if valid { status } else { 400 };
            write!(stream, "HTTP/1.1 {reply_status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            valid
        });
        let root = tempfile::tempdir().unwrap();
        let path = root.path();
        let core = ImCore::open(
            ImCoreConfig {
                service_base_url: ServiceEndpoint::parse(&base).unwrap(),
                did_domain: "identity.example".into(),
                client_version_info: None,
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: None,
                ca_bundle: None,
                transport_policy: MessageTransportPolicy::HttpOnly,
            },
            ImCorePaths {
                identities: IdentityRegistryPaths {
                    identity_root_dir: path.join("identities"),
                    registry_path: path.join("identities/registry.json"),
                    default_identity_path: None,
                },
                local_state: LocalStatePaths {
                    sqlite_path: path.join("state.sqlite"),
                },
                runtime: RuntimePaths {
                    cache_dir: path.join("cache"),
                    temp_dir: path.join("tmp"),
                },
            },
        )
        .await
        .unwrap();
        let result = core.identities().creation_capabilities_async().await;
        assert!(
            server.join().unwrap(),
            "creation discovery sent an invalid HTTP method or authentication"
        );
        assert_eq!(result.unwrap().did_methods, expected);
    }
}
