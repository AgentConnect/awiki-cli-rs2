use super::*;
use std::time::Duration;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};

const PRODUCTS: [(&str, &str); 5] = [
    ("awiki-me", "1.0.58"),
    ("awiki-cli", "1.0.58"),
    ("awiki-daemon", "0.1.93"),
    ("awiki-dsh", "0.3.17"),
    ("awiki-gateway", "0.1.0"),
];

fn fixture(endpoint: &str, product: &str, version: &str) -> (tempfile::TempDir, crate::ImClient) {
    let root = tempfile::tempdir().unwrap();
    let mut config = crate::ImCoreConfig::new(
        crate::ServiceEndpoint::parse(endpoint).unwrap(),
        "example.test",
    )
    .unwrap();
    config.client_version_info =
        Some(crate::ClientVersionInfo::new(product, "0815", version, None).unwrap());
    let core = crate::ImCore::new(
        config,
        crate::ImCorePaths {
            identities: crate::IdentityRegistryPaths {
                identity_root_dir: root.path().join("identities"),
                registry_path: root.path().join("identities/registry.json"),
                default_identity_path: None,
            },
            local_state: crate::LocalStatePaths {
                sqlite_path: root.path().join("state.sqlite"),
            },
            runtime: crate::RuntimePaths {
                cache_dir: root.path().join("cache"),
                temp_dir: root.path().join("tmp"),
            },
        },
    )
    .unwrap();
    let material = anp::authentication::create_did_wba_document(
        "example.test",
        anp::authentication::DidDocumentOptions::default(),
    )
    .unwrap();
    let client = core
        .client_with_identity_material(crate::identity::HostedIdentityMaterial {
            identity_id: "client-version-fixture".to_owned(),
            did: material.did_document["id"].as_str().unwrap().to_owned(),
            handle: None,
            display_name: None,
            did_document: material.did_document.clone(),
            default_signing_private_key_pem: material.keys["key-1"].private_key_pem.clone(),
            e2ee_agreement_private_key_pem: None,
            auth_token: None,
        })
        .unwrap();
    (root, client)
}

async fn check_upgrade(listener: tokio::net::TcpListener, expected: String) {
    let (stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let socket = tokio_tungstenite::accept_hdr_async(
        stream,
        |request: &Request, response: Response| -> Result<Response, ErrorResponse> {
            assert_eq!(request.uri().path(), "/im/ws");
            assert_eq!(request.headers()["authorization"], "Bearer fixture-token");
            assert_eq!(request.headers()["x-awiki-client-version"], expected);
            assert_eq!(
                request
                    .headers()
                    .get_all("x-awiki-client-version")
                    .iter()
                    .count(),
                1
            );
            Ok(response)
        },
    )
    .await
    .unwrap();
    drop(socket);
}

#[tokio::test]
async fn client_version_products_reach_async_websocket_upgrade() {
    for (product, version) in PRODUCTS {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let (root, client) = fixture(&base, product, version);
        let server = tokio::spawn(check_upgrade(listener, format!("{product}/0815/{version}")));
        let endpoints = realtime_client_endpoints(&base).unwrap();
        let transport = tokio::time::timeout(
            Duration::from_secs(5),
            connect_async_websocket_session_with_token(
                &client,
                &endpoints,
                "fixture-token",
                false,
                None,
            ),
        )
        .await
        .unwrap()
        .unwrap();
        drop(transport);
        server.await.unwrap();
        drop(client);
        drop(root);
    }
}

#[cfg(feature = "blocking")]
#[tokio::test]
async fn client_version_products_reach_blocking_websocket_upgrade() {
    for (product, version) in PRODUCTS {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(check_upgrade(listener, format!("{product}/0815/{version}")));
        tokio::task::spawn_blocking(move || {
            let (root, client) = fixture(&base, product, version);
            let endpoints = realtime_client_endpoints(&base).unwrap();
            let transport = connect_native_websocket_session_with_token(
                &client,
                &endpoints,
                "fixture-token",
                false,
                None,
            )
            .unwrap();
            drop(transport);
            drop(client);
            drop(root);
        })
        .await
        .unwrap();
        server.await.unwrap();
    }
}
