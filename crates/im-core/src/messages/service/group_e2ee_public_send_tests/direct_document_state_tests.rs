use super::*;
use crate::internal::message_runtime::send_lifecycle;
use crate::internal::secure_direct::document_state;
use crate::internal::wire::sync_v2::direct_e2ee_state::{
    IdentityObservation, IdentityObservations,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use sha2::{Digest, Sha256};

#[tokio::test]
async fn cached_identity_requires_matching_authority_hash_scope_and_live_sync() {
    let fixture = Fixture::vnext();
    let core = crate::core::ImCore::new_with_options(
        fixture.config("http://127.0.0.1:9".into()),
        fixture.paths(),
        fixture.vnext_open_options(),
    )
    .unwrap();
    let client = core
        .client(crate::identity::IdentitySelector::LocalAlias(
            "alice".into(),
        ))
        .unwrap();
    let did = client.did().as_str();
    assert!(did.starts_with("did:wba:awiki.test:user:"));
    let document = client
        .runtime()
        .key_provider
        .optional_did_document()
        .unwrap()
        .unwrap();
    let hash = format!(
        "sha256:{}",
        URL_SAFE_NO_PAD.encode(Sha256::digest(
            serde_json_canonicalizer::to_vec(&document).unwrap()
        ))
    );
    document_state::remember(&client, did, &document).unwrap();
    assert!(document_state::get(&client, did).unwrap().is_none());
    let guard = send_lifecycle::ConnectionGuard::connected(&client).unwrap();
    let observation = IdentityObservations {
        schema_version: 1,
        complete: true,
        identities: vec![IdentityObservation {
            did: did.into(),
            document_version: "1".into(),
            document_hash: hash,
        }],
    };
    document_state::observe_async(
        &client,
        Some(&observation),
        send_lifecycle::connection_token(&client),
        true,
    )
    .await
    .unwrap();
    assert_eq!(
        document_state::get(&client, did).unwrap(),
        Some(document.clone())
    );
    document_state::observe_hint(
        &client,
        &json!({"method":"sync.changed","params":{"domains":["message"],"reason":"e2ee_identity_changed"}}),
    );
    assert!(document_state::get(&client, did).unwrap().is_none());
    document_state::observe_async(
        &client,
        Some(&observation),
        send_lifecycle::connection_token(&client),
        true,
    )
    .await
    .unwrap();
    assert_eq!(
        document_state::get(&client, did).unwrap(),
        Some(document.clone()),
        "unchanged hashes restore reusable documents without another fetch"
    );
    let mut mismatch = document.clone();
    mismatch["test_changed"] = json!(true);
    assert!(document_state::remember(&client, did, &mismatch).is_err());
    assert!(
        document_state::get(&client, did).unwrap().is_none(),
        "a contradictory resolved document invalidates prior observations"
    );
    document_state::observe_async(
        &client,
        Some(&observation),
        send_lifecycle::connection_token(&client),
        true,
    )
    .await
    .unwrap();
    drop(send_lifecycle::ReceiveGuard::new(&client));
    assert!(
        document_state::get(&client, did).unwrap().is_none(),
        "cancelled receive invalidates its observation token"
    );
    assert!(
        document_state::get(&client, "did:web:awiki.test:user:alice")
            .unwrap()
            .is_none()
    );
    assert!(
        document_state::get(&client, "did:wba:other.test:user:alice")
            .unwrap()
            .is_none()
    );
    drop(guard);
    assert!(document_state::get(&client, did).unwrap().is_none());
}
