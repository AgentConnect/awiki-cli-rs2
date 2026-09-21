use super::*;
use crate::internal::directory_runtime::DirectoryRuntime;
use crate::internal::transport::{AsyncRpcTransport, RpcTransport};
use serde_json::json;

struct PublicDirectory {
    did: &'static str,
    hint_did: &'static str,
}
impl PublicDirectory {
    fn value(&self) -> Value {
        json!({"handle":"peer.remote.test", "did":self.did,"status":"active","binding_generation":"2","user_id":"ignored-provider-private"})
    }
    fn rpc_value(&self, method: &str, params: &Value) -> crate::ImResult<Value> {
        if method == "lookup" && params.get("did").is_some() {
            return Ok(
                json!({"handle":"peer.remote.test","did":self.hint_did,"domain":"remote.test","user_id":"wrong-home-user","status":"active","binding_generation":"2"}),
            );
        }
        Err(crate::ImError::unsupported(
            "home-directory-has-no-foreign-profile",
        ))
    }
}
impl RpcTransport for PublicDirectory {
    fn rpc(&mut self, _: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        self.rpc_value(method, &params)
    }
    fn directory_get_json_url(
        &mut self,
        url: &str,
        headers: BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        assert!(url.starts_with("https://remote.test/"));
        assert!(headers.is_empty());
        Ok(self.value())
    }
}
impl AsyncRpcTransport for PublicDirectory {
    async fn rpc(&mut self, _: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        self.rpc_value(method, &params)
    }
    async fn directory_get_json_url(
        &mut self,
        url: &str,
        headers: BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        RpcTransport::directory_get_json_url(self, url, headers)
    }
}
const DID: &str = "did:wba:remote.test:user:peer";
fn transport() -> PublicDirectory {
    PublicDirectory {
        did: DID,
        hint_did: DID,
    }
}

#[test]
fn foreign_persona_directory_query_then_direct_share_authority() {
    let fixture = super::tests::Fixture::new("foreign-directory");
    let client = fixture.client();
    let lookup = DirectoryRuntime::new(&client, transport())
        .lookup_handle(crate::ids::Handle::parse("peer.remote.test", "").unwrap())
        .unwrap();
    assert_eq!(lookup.user_id, "peer.remote.test");
    crate::directory::project_handle_lookup(&client, &lookup).unwrap();
    let direct =
        finish_public_direct_resolution(&client, "peer.remote.test", transport().value()).unwrap();
    assert_eq!(direct.authority_subject_id, lookup.user_id);
    let resolved = DirectoryRuntime::new(&client, transport())
        .resolve_peer(crate::ids::PeerRef::parse("peer.remote.test", "").unwrap())
        .unwrap();
    assert_eq!(
        resolved.resolution.conversation_id,
        lookup.direct_conversation_id()
    );
}

#[tokio::test]
async fn foreign_persona_async_directory_and_inbound_verify_provider() {
    let fixture = super::tests::Fixture::new("foreign-directory-async");
    let client = fixture.client();
    let lookup = DirectoryRuntime::new(&client, transport())
        .lookup_handle_async(crate::ids::Handle::parse("peer.remote.test", "").unwrap())
        .await
        .unwrap();
    let inbound = crate::internal::directory_runtime::lookup_handle_by_did_for_projection_async(
        &client,
        &mut transport(),
        &crate::ids::Did::parse(DID).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(lookup, inbound);
    let mut changed = PublicDirectory {
        did: "did:wba:remote.test:user:other",
        hint_did: DID,
    };
    assert!(matches!(
        crate::internal::directory_runtime::lookup_handle_by_did_for_projection_async(
            &client,
            &mut changed,
            &crate::ids::Did::parse(DID).unwrap()
        )
        .await,
        Err(crate::ImError::IdentityBindingConflict { .. })
    ));
    let resolved = DirectoryRuntime::new(&client, transport())
        .resolve_peer_async(crate::ids::PeerRef::parse("peer.remote.test", "").unwrap())
        .await
        .unwrap();
    assert_eq!(
        resolved.resolution.conversation_id,
        lookup.direct_conversation_id()
    );
}
