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

struct ProfileDirectory {
    public: PublicDirectory,
    profile: Option<Value>,
    rpc_profile: Value,
}

impl RpcTransport for ProfileDirectory {
    fn rpc(&mut self, _: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        match method {
            "get_public_profile" => Ok(self.rpc_profile.clone()),
            "resolve" => Ok(json!({"did": DID})),
            _ => self.public.rpc_value(method, &params),
        }
    }

    fn directory_get_json_url(
        &mut self,
        url: &str,
        headers: BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        let mut raw = RpcTransport::directory_get_json_url(&mut self.public, url, headers)?;
        if let Some(profile) = self.profile.clone() {
            raw["profile"] = profile;
        }
        Ok(raw)
    }
}

impl AsyncRpcTransport for ProfileDirectory {
    async fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        RpcTransport::rpc(self, endpoint, method, params)
    }

    async fn directory_get_json_url(
        &mut self,
        url: &str,
        headers: BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        RpcTransport::directory_get_json_url(self, url, headers)
    }
}

fn profile_transport(profile: Option<Value>) -> ProfileDirectory {
    ProfileDirectory {
        public: transport(),
        profile,
        rpc_profile: json!({"did": DID, "handle": null, "display_name": null}),
    }
}

fn owner_handle_fixture(prefix: &str) -> super::tests::Fixture {
    let fixture = super::tests::Fixture::new(prefix);
    let path = fixture.root.join("identities/registry.json");
    let mut registry: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    registry["identities"][0]["handle"] = json!("alice.awiki.test");
    std::fs::write(path, serde_json::to_vec(&registry).unwrap()).unwrap();
    fixture
}

#[tokio::test]
async fn public_profile_missing_handle_never_inherits_actor_sync_or_async() {
    let fixture = owner_handle_fixture("public-profile-missing-handle");
    let client = fixture.client();
    assert_eq!(client.handle().unwrap().as_str(), "alice.awiki.test");
    let peer = crate::ids::PeerRef::parse("peer.remote.test", "").unwrap();
    let sync = DirectoryRuntime::new(&client, profile_transport(None))
        .resolve_peer(peer.clone())
        .unwrap();
    let asynchronous = DirectoryRuntime::new(&client, profile_transport(None))
        .resolve_peer_async(peer)
        .await
        .unwrap();
    assert_eq!(sync, asynchronous);
    let profile = sync.resolution.profile.unwrap();
    assert_eq!(profile.subject.as_str(), DID);
    assert_eq!(profile.handle.unwrap().as_str(), "peer.remote.test");
    assert_eq!(profile.display_name, None);

    let subject = crate::directory::IdentitySubject::Did(crate::ids::Did::parse(DID).unwrap());
    let sync = DirectoryRuntime::new(&client, profile_transport(None))
        .public_profile(subject.clone())
        .unwrap();
    let asynchronous = DirectoryRuntime::new(&client, profile_transport(None))
        .public_profile_async(subject)
        .await
        .unwrap();
    assert_eq!(sync, asynchronous);
    assert_eq!(sync.profile.subject.as_str(), DID);
    assert_eq!(sync.profile.handle, None);
    assert_eq!(sync.handle, None);

    // Only the own-profile reader retains the legacy actor fallback.
    let own = crate::internal::profile_runtime::profile_from_value(&client, &json!({})).unwrap();
    assert_eq!(own.subject, *client.did());
    assert_eq!(own.handle.as_ref(), client.handle());
}

#[tokio::test]
async fn foreign_wns_display_profile_preserves_binding_and_omitted_handle() {
    let fixture = owner_handle_fixture("foreign-display-profile");
    let client = fixture.client();
    for handle in [Value::Null, json!("peer"), json!("peer.remote.test")] {
        let raw = json!({"subject_did": DID, "handle": handle, "display_name": "Peer Nickname"});
        let peer = crate::ids::PeerRef::parse("peer.remote.test", "").unwrap();
        let sync = DirectoryRuntime::new(&client, profile_transport(Some(raw.clone())))
            .resolve_peer(peer.clone())
            .unwrap();
        let asynchronous = DirectoryRuntime::new(&client, profile_transport(Some(raw)))
            .resolve_peer_async(peer)
            .await
            .unwrap();
        assert_eq!(sync, asynchronous);
        assert!(
            sync.public_profile.is_none(),
            "WNS display should avoid the partial home profile"
        );
        let lookup = sync.handle_lookup.unwrap();
        assert_eq!(lookup.user_id, "peer.remote.test");
        assert_eq!(lookup.binding_generation.as_deref(), Some("2"));
        let profile = sync.resolution.profile.unwrap();
        assert_eq!(profile.subject.as_str(), DID);
        assert_eq!(profile.handle.unwrap().as_str(), "peer.remote.test");
        assert_eq!(profile.display_name.as_deref(), Some("Peer Nickname"));
        for by_did in [false, true] {
            let selector = if by_did { DID } else { "peer.remote.test" };
            let public = json!({"subject_did": DID, "display_name": "Peer Nickname"});
            let did_result =
                DirectoryRuntime::new(&client, profile_transport(Some(public.clone())))
                    .resolve_peer(crate::ids::PeerRef::parse(selector, "").unwrap())
                    .unwrap();
            let async_result = DirectoryRuntime::new(&client, profile_transport(Some(public)))
                .resolve_peer_async(crate::ids::PeerRef::parse(selector, "").unwrap())
                .await
                .unwrap();
            assert_eq!(did_result, async_result);
            assert_eq!(
                did_result
                    .resolution
                    .profile
                    .unwrap()
                    .display_name
                    .as_deref(),
                Some("Peer Nickname")
            );
            assert_eq!(
                did_result.resolution.conversation_id,
                lookup.direct_conversation_id()
            );
        }
        crate::directory::project_handle_lookup(&client, &lookup).unwrap();
        assert_eq!(
            sync.resolution.conversation_id,
            lookup.direct_conversation_id()
        );
    }
}

#[tokio::test]
async fn mismatched_public_profile_fails_closed_without_relabeling_subject() {
    let fixture = owner_handle_fixture("public-profile-conflict");
    let client = fixture.client();
    let peer = crate::ids::PeerRef::parse("peer.remote.test", "").unwrap();
    for raw in [
        json!({"did": client.did().as_str(), "handle": "peer.remote.test"}),
        json!({"did": DID, "subject_did": client.did().as_str()}),
        json!({"did": DID, "handle": "alice.awiki.test"}),
    ] {
        let mut sync = profile_transport(None);
        sync.rpc_profile = raw.clone();
        assert!(matches!(
            DirectoryRuntime::new(&client, sync).resolve_peer(peer.clone()),
            Err(crate::ImError::IdentityBindingConflict { .. })
        ));
        let mut asynchronous = profile_transport(None);
        asynchronous.rpc_profile = raw;
        assert!(matches!(
            DirectoryRuntime::new(&client, asynchronous)
                .resolve_peer_async(peer.clone())
                .await,
            Err(crate::ImError::IdentityBindingConflict { .. })
        ));
    }
}

#[test]
fn invalid_wns_display_is_discarded_without_changing_verified_route() {
    let fixture = owner_handle_fixture("wns-display-conflict");
    let client = fixture.client();
    for raw in [
        json!({"subject_did": client.did().as_str(), "display_name": "Wrong"}),
        json!({"subject_did": DID, "handle": "alice.awiki.test", "display_name": "Wrong"}),
        json!({"subject_did": DID, "did": client.did().as_str(), "display_name": "Wrong"}),
        json!({"subject_did": DID, "profile_version": 1, "display_name": "Wrong"}),
        json!({"display_name": "Unbound"}),
    ] {
        let result = DirectoryRuntime::new(&client, profile_transport(Some(raw)))
            .resolve_peer(crate::ids::PeerRef::parse("peer.remote.test", "").unwrap())
            .unwrap();
        assert!(!result.resolution.warnings.is_empty());
        assert_eq!(result.resolution.did.as_str(), DID);
        assert_eq!(
            result.resolution.handle.unwrap().as_str(),
            "peer.remote.test"
        );
        let profile = result.resolution.profile.unwrap();
        assert_eq!(profile.display_name, None);
        assert_eq!(profile.handle.unwrap().as_str(), "peer.remote.test");
    }
}
