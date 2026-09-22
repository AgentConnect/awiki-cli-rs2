use super::*;
use serde_json::json;

const HANDLE: &str = "alice.provider.example";
const WEB_DID: &str = "did:web:identity.example:alice";

fn binding(did: &str) -> Value {
    json!({"handle": HANDLE, "did": did, "status": "active", "binding_generation": "1",
        "user_id": "untrusted-provider-private-id"})
}

fn assert_persisted(fixture: &super::tests::Fixture, did: &str) {
    // A new Core/client sees the same owner-bound verified Persona, not a DID alias.
    let client = fixture.client();
    let db = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )
    .unwrap();
    let persona = crate::internal::local_state::peer_personas::resolve_by_did(&db, "alice-id", did)
        .unwrap()
        .expect("verified public binding must persist for later inbound sync");
    let expected = authoritative_lookup_from_public_document(HANDLE, &binding(did)).unwrap();
    assert_eq!(persona.conversation_id, expected.direct_conversation_id());
    let route = crate::internal::local_state::direct_peer_routes::get(
        &db,
        "alice-id",
        &persona.conversation_id,
    )
    .unwrap()
    .unwrap();
    assert_eq!(route.current_did, did);
    assert_eq!(route.peer_user_id, HANDLE);
    assert_eq!(route.full_handle, HANDLE);
    assert!(
        crate::internal::local_state::peer_personas::resolve_by_did(&db, "other-owner", did)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn public_handle_resolution_persists_verified_route_before_first_message() {
    for did in [WEB_DID, "did:wba:provider.example:alice"] {
        let fixture = super::tests::Fixture::new("public-projection");
        let client = fixture.client();
        finish_public_direct_resolution(&client, HANDLE, binding(did)).unwrap();
        drop(client);
        assert_persisted(&fixture, did);
        std::fs::remove_dir_all(&fixture.root).unwrap();
    }
}

#[tokio::test]
async fn public_handle_resolution_async_persists_only_valid_owner_binding() {
    let fixture = super::tests::Fixture::new("public-async-projection");
    let client = fixture.client();
    let mut inactive = binding(WEB_DID);
    inactive["status"] = json!("inactive");
    assert!(
        finish_public_direct_resolution_async(&client, HANDLE, inactive)
            .await
            .is_err()
    );
    finish_public_direct_resolution_async(&client, HANDLE, binding(WEB_DID))
        .await
        .unwrap();
    finish_public_direct_resolution_async(&client, HANDLE, binding(WEB_DID))
        .await
        .unwrap();
    drop(client);
    assert_persisted(&fixture, WEB_DID);
    std::fs::remove_dir_all(&fixture.root).unwrap();
}
