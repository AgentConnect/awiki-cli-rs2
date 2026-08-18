use serde_json::Value;

use crate::internal::auth::session::{AsyncSessionProvider, SessionProvider};
use crate::internal::transport::{AsyncAuthenticatedRpcTransport, AuthenticatedRpcTransport};

pub(crate) const MESSAGE_RPC_ENDPOINT: &str = "/im/rpc";

pub(crate) struct GroupReadRuntime<'a, P, T> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
}

impl<'a, P, T> GroupReadRuntime<'a, P, T> {
    pub(crate) fn new(
        client: &'a crate::core::ImClient,
        session_provider: P,
        transport: T,
    ) -> Self {
        Self {
            client,
            session_provider,
            transport,
        }
    }
}

impl<'a, P, T> GroupReadRuntime<'a, P, T>
where
    P: SessionProvider,
    T: AuthenticatedRpcTransport,
{
    pub(crate) fn get(
        mut self,
        group: crate::ids::GroupRef,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session()?;
        let params = crate::internal::wire::group::build_group_get_rpc_params(
            self.client.did().as_str(),
            group.as_str(),
        )?;
        self.group_rpc("group.get", params)
    }

    pub(crate) fn get_with_policy(
        mut self,
        group: crate::ids::GroupRef,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session()?;
        let local_params = crate::internal::wire::group::build_group_get_rpc_params(
            self.client.did().as_str(),
            group.as_str(),
        )?;
        let local =
            self.transport
                .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "group.get", local_params)?;
        let operation_id = format!(
            "group-read-{}",
            crate::internal::wire::common::generate_operation_id()
        );
        let info_params = crate::internal::wire::group::build_group_get_info_rpc_params(
            self.client.did().as_str(),
            group.as_str(),
            &operation_id,
            true,
        )?;
        let authoritative = self.transport.authenticated_rpc(
            MESSAGE_RPC_ENDPOINT,
            "group.get_info",
            info_params,
        )?;
        let merged = merge_authoritative_group_policy(group.as_str(), local, authoritative)?;
        Ok(crate::groups::GroupReadResult::from_raw_response(
            merged,
            Vec::new(),
        ))
    }

    pub(crate) fn list(
        mut self,
        request: crate::groups::GroupListRequest,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session()?;
        let params = crate::internal::wire::group::build_group_list_rpc_params(
            self.client.did().as_str(),
            page_limit(request.limit, 50),
            request.cursor.as_ref().map(crate::ids::Cursor::as_str),
        );
        let result = self.group_rpc("group.list", params)?;
        validate_relaxed_collection_page(&result, "groups", result.groups.len())?;
        Ok(result)
    }

    pub(crate) fn members(
        mut self,
        request: crate::groups::GroupMembersRequest,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session()?;
        let params = crate::internal::wire::group::build_group_members_rpc_params(
            self.client.did().as_str(),
            request.group.as_str(),
            page_limit(request.limit, 100),
            request.cursor.as_ref().map(crate::ids::Cursor::as_str),
        )?;
        let result = self.group_rpc("group.list_members", params)?;
        validate_relaxed_collection_page(&result, "members", result.members.len())?;
        Ok(result)
    }

    pub(crate) fn messages(
        mut self,
        request: crate::groups::GroupMessagesRequest,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session()?;
        let params = crate::internal::wire::group::build_group_messages_rpc_params_for_client(
            self.client,
            request.group.as_str(),
            page_limit(request.limit, 50),
            request.cursor.as_ref().map(crate::ids::Cursor::as_str),
            0,
        )?;
        let mut raw = self.transport.authenticated_rpc(
            MESSAGE_RPC_ENDPOINT,
            "group.list_messages",
            params,
        )?;
        project_group_e2ee_messages(self.client, &mut raw);
        Ok(crate::groups::GroupReadResult::from_raw_response(
            raw,
            Vec::new(),
        ))
    }

    fn ensure_group_session(&self) -> crate::ImResult<crate::auth::SessionBundle> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)
    }

    fn group_rpc(
        &mut self,
        method: &str,
        params: Value,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        let raw = self
            .transport
            .authenticated_rpc(MESSAGE_RPC_ENDPOINT, method, params)?;
        Ok(crate::groups::GroupReadResult::from_raw_response(
            raw,
            Vec::new(),
        ))
    }
}

impl<'a, P, T> GroupReadRuntime<'a, P, T>
where
    P: AsyncSessionProvider,
    T: AsyncAuthenticatedRpcTransport,
{
    pub(crate) async fn get_async(
        mut self,
        group: crate::ids::GroupRef,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session_async().await?;
        let params = crate::internal::wire::group::build_group_get_rpc_params(
            self.client.did().as_str(),
            group.as_str(),
        )?;
        self.group_rpc_async("group.get", params).await
    }

    pub(crate) async fn get_with_policy_async(
        mut self,
        group: crate::ids::GroupRef,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session_async().await?;
        let local_params = crate::internal::wire::group::build_group_get_rpc_params(
            self.client.did().as_str(),
            group.as_str(),
        )?;
        let local = self
            .transport
            .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "group.get", local_params)
            .await?;
        let operation_id = format!(
            "group-read-{}",
            crate::internal::wire::common::generate_operation_id()
        );
        let info_params = crate::internal::wire::group::build_group_get_info_rpc_params(
            self.client.did().as_str(),
            group.as_str(),
            &operation_id,
            true,
        )?;
        let authoritative = self
            .transport
            .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "group.get_info", info_params)
            .await?;
        let merged = merge_authoritative_group_policy(group.as_str(), local, authoritative)?;
        Ok(crate::groups::GroupReadResult::from_raw_response(
            merged,
            Vec::new(),
        ))
    }

    pub(crate) async fn list_async(
        mut self,
        request: crate::groups::GroupListRequest,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session_async().await?;
        let params = crate::internal::wire::group::build_group_list_rpc_params(
            self.client.did().as_str(),
            page_limit(request.limit, 50),
            request.cursor.as_ref().map(crate::ids::Cursor::as_str),
        );
        let result = self.group_rpc_async("group.list", params).await?;
        validate_relaxed_collection_page(&result, "groups", result.groups.len())?;
        Ok(result)
    }

    pub(crate) async fn members_async(
        mut self,
        request: crate::groups::GroupMembersRequest,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session_async().await?;
        let params = crate::internal::wire::group::build_group_members_rpc_params(
            self.client.did().as_str(),
            request.group.as_str(),
            page_limit(request.limit, 100),
            request.cursor.as_ref().map(crate::ids::Cursor::as_str),
        )?;
        let result = self.group_rpc_async("group.list_members", params).await?;
        validate_relaxed_collection_page(&result, "members", result.members.len())?;
        Ok(result)
    }

    pub(crate) async fn messages_async(
        mut self,
        request: crate::groups::GroupMessagesRequest,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session_async().await?;
        let params = crate::internal::wire::group::build_group_messages_rpc_params_for_client(
            self.client,
            request.group.as_str(),
            page_limit(request.limit, 50),
            request.cursor.as_ref().map(crate::ids::Cursor::as_str),
            0,
        )?;
        let mut raw = self
            .transport
            .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "group.list_messages", params)
            .await?;
        project_group_e2ee_messages_async(self.client, &mut raw).await;
        Ok(crate::groups::GroupReadResult::from_raw_response(
            raw,
            Vec::new(),
        ))
    }

    async fn ensure_group_session_async(&self) -> crate::ImResult<crate::auth::SessionBundle> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)
            .await
    }

    async fn group_rpc_async(
        &mut self,
        method: &str,
        params: Value,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        let raw = self
            .transport
            .authenticated_rpc(MESSAGE_RPC_ENDPOINT, method, params)
            .await?;
        Ok(crate::groups::GroupReadResult::from_raw_response(
            raw,
            Vec::new(),
        ))
    }
}

#[cfg(feature = "group-e2ee")]
fn project_group_e2ee_messages(client: &crate::core::ImClient, raw: &mut Value) {
    crate::internal::message_runtime::read::project_group_e2ee_messages(client, raw);
}

#[cfg(not(feature = "group-e2ee"))]
fn project_group_e2ee_messages(_client: &crate::core::ImClient, _raw: &mut Value) {}

#[cfg(feature = "group-e2ee")]
async fn project_group_e2ee_messages_async(client: &crate::core::ImClient, raw: &mut Value) {
    crate::internal::message_runtime::read::project_group_e2ee_messages_async(client, raw).await;
}

#[cfg(not(feature = "group-e2ee"))]
async fn project_group_e2ee_messages_async(_client: &crate::core::ImClient, _raw: &mut Value) {}

fn merge_authoritative_group_policy(
    expected_group_did: &str,
    mut local: Value,
    authoritative: Value,
) -> crate::ImResult<Value> {
    let authoritative = authoritative
        .as_object()
        .ok_or_else(|| crate::ImError::Serialization {
            detail: "P4 group.get_info result must be an object".to_owned(),
        })?;
    let returned_group_did = authoritative
        .get("group_did")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| crate::ImError::Serialization {
            detail: "P4 group.get_info result is missing group_did".to_owned(),
        })?;
    if returned_group_did != expected_group_did {
        return Err(crate::ImError::PermissionDenied);
    }
    let group_state_version = authoritative
        .get("group_state_version")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| crate::ImError::Serialization {
            detail: "P4 group.get_info result is missing group_state_version".to_owned(),
        })?;
    if !authoritative
        .get("group_profile")
        .is_some_and(Value::is_object)
    {
        return Err(crate::ImError::Serialization {
            detail: "P4 group.get_info result is missing or has malformed group_profile".to_owned(),
        });
    }
    let group_policy = authoritative
        .get("group_policy")
        .filter(|value| value.is_object())
        .cloned()
        .ok_or_else(|| crate::ImError::Serialization {
            detail: "P4 group.get_info result is missing group_policy".to_owned(),
        })?;
    let local_object = local
        .as_object_mut()
        .ok_or_else(|| crate::ImError::Serialization {
            detail: "domain-local group.get result must be an object".to_owned(),
        })?;
    local_object.insert(
        "group_did".to_owned(),
        Value::String(returned_group_did.to_owned()),
    );
    local_object.insert(
        "group_state_version".to_owned(),
        Value::String(group_state_version.to_owned()),
    );
    local_object.insert("group_policy".to_owned(), group_policy);
    Ok(local)
}

fn page_limit(limit: crate::ids::PageLimit, fallback: i64) -> i64 {
    if limit.0 == 0 {
        fallback
    } else {
        i64::from(limit.0)
    }
}

fn validate_relaxed_collection_page(
    result: &crate::groups::GroupReadResult,
    collection: &str,
    parsed_count: usize,
) -> crate::ImResult<()> {
    let raw = result
        .raw_response()
        .ok_or(crate::ImError::InventoryIncomplete)?;
    let raw_count = raw
        .get(collection)
        .and_then(Value::as_array)
        .map(Vec::len)
        .ok_or(crate::ImError::InventoryIncomplete)?;
    if raw_count != parsed_count
        || (raw.get("total").is_some() && result.total.is_none())
        || (raw.get("next_cursor").is_some() && result.next_cursor.is_none())
    {
        return Err(crate::ImError::InventoryIncomplete);
    }
    match raw.get("has_more") {
        Some(Value::Bool(true)) if result.next_cursor.is_some() => Ok(()),
        Some(Value::Bool(false)) if raw.get("next_cursor").is_none() => Ok(()),
        None if result
            .total
            .is_some_and(|total| total as usize == raw_count)
            && raw.get("next_cursor").is_none() =>
        {
            Ok(())
        }
        _ => Err(crate::ImError::InventoryIncomplete),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::auth::session::SessionProvider;
    use crate::internal::transport::AuthenticatedRpcTransport;
    use serde_json::{json, Value};
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::fs;
    use std::path::PathBuf;
    use std::rc::Rc;

    #[test]
    fn groups_read_runtime_builds_get_list_members_and_messages_rpc() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let group = crate::ids::GroupRef::parse("did:example:group").unwrap();

        GroupReadRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"group_did":"did:example:group"}),
            },
        )
        .get(group.clone())
        .unwrap();

        GroupReadRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"groups":[],"total":0}),
            },
        )
        .list(crate::groups::GroupListRequest {
            limit: crate::ids::PageLimit(25),
            cursor: Some(crate::ids::Cursor::parse("groups-page-2").unwrap()),
        })
        .unwrap();

        GroupReadRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"members":[],"total":0}),
            },
        )
        .members(crate::groups::GroupMembersRequest {
            group: group.clone(),
            limit: crate::ids::PageLimit(10),
            cursor: Some(crate::ids::Cursor::parse("members-page-2").unwrap()),
        })
        .unwrap();

        GroupReadRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"messages":[]}),
            },
        )
        .messages(crate::groups::GroupMessagesRequest {
            group,
            limit: crate::ids::PageLimit(5),
            cursor: Some(crate::ids::Cursor::parse("42").unwrap()),
        })
        .unwrap();

        let calls = calls.borrow();
        assert_eq!(calls.len(), 4);
        assert_eq!(calls[0].method, "group.get");
        assert_eq!(calls[0].endpoint, MESSAGE_RPC_ENDPOINT);
        assert_eq!(calls[0].params["meta"]["sender_did"], "did:example:alice");
        assert_eq!(
            calls[0].params["meta"]["target"],
            json!({"kind":"group","did":"did:example:group"})
        );
        assert_eq!(calls[0].params["body"]["group_did"], "did:example:group");
        assert_eq!(calls[1].method, "group.list");
        assert_eq!(calls[1].params["body"]["limit"], 25);
        assert_eq!(calls[1].params["body"]["cursor"], "groups-page-2");
        assert_eq!(calls[2].method, "group.list_members");
        assert_eq!(calls[2].params["body"]["limit"], 10);
        assert_eq!(calls[2].params["body"]["cursor"], "members-page-2");
        assert_eq!(calls[3].method, "group.list_messages");
        assert_eq!(calls[3].params["body"]["limit"], 5);
        assert_eq!(calls[3].params["body"]["since_seq"], "42");
    }

    #[tokio::test]
    async fn groups_read_runtime_builds_get_list_members_and_messages_rpc_async() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let group = crate::ids::GroupRef::parse("did:example:group").unwrap();

        GroupReadRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"group_did":"did:example:group"}),
            },
        )
        .get_async(group.clone())
        .await
        .unwrap();

        GroupReadRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"groups":[],"total":0}),
            },
        )
        .list_async(crate::groups::GroupListRequest {
            limit: crate::ids::PageLimit(25),
            cursor: Some(crate::ids::Cursor::parse("groups-page-2").unwrap()),
        })
        .await
        .unwrap();

        GroupReadRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"members":[],"total":0}),
            },
        )
        .members_async(crate::groups::GroupMembersRequest {
            group: group.clone(),
            limit: crate::ids::PageLimit(10),
            cursor: Some(crate::ids::Cursor::parse("members-page-2").unwrap()),
        })
        .await
        .unwrap();

        GroupReadRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"messages":[]}),
            },
        )
        .messages_async(crate::groups::GroupMessagesRequest {
            group,
            limit: crate::ids::PageLimit(5),
            cursor: Some(crate::ids::Cursor::parse("42").unwrap()),
        })
        .await
        .unwrap();

        let calls = calls.borrow();
        assert_eq!(calls.len(), 4);
        assert_eq!(calls[0].method, "group.get");
        assert_eq!(calls[0].endpoint, MESSAGE_RPC_ENDPOINT);
        assert_eq!(calls[0].params["meta"]["sender_did"], "did:example:alice");
        assert_eq!(
            calls[0].params["meta"]["target"],
            json!({"kind":"group","did":"did:example:group"})
        );
        assert_eq!(calls[0].params["body"]["group_did"], "did:example:group");
        assert_eq!(calls[1].method, "group.list");
        assert_eq!(calls[1].params["body"]["limit"], 25);
        assert_eq!(calls[1].params["body"]["cursor"], "groups-page-2");
        assert_eq!(calls[2].method, "group.list_members");
        assert_eq!(calls[2].params["body"]["limit"], 10);
        assert_eq!(calls[2].params["body"]["cursor"], "members-page-2");
        assert_eq!(calls[3].method, "group.list_messages");
        assert_eq!(calls[3].params["body"]["limit"], 5);
        assert_eq!(calls[3].params["body"]["since_seq"], "42");
    }

    #[test]
    #[cfg(feature = "group-e2ee")]
    fn get_with_policy_merges_p4_policy_without_losing_local_membership() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let responses = Rc::new(RefCell::new(VecDeque::from([
            json!({
                "group_snapshot": {
                    "group_did": "did:example:group",
                    "my_role": "owner",
                    "membership_status": "active",
                    "required_security_profile": "transport-protected"
                }
            }),
            json!({
                "group_did": "did:example:group",
                "group_state_version": "state-2",
                "group_profile": {"display_name": "Remote group"},
                "group_policy": {"message_security_profile": "group-e2ee"}
            }),
        ])));

        let result = GroupReadRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            SequencedRecordingTransport {
                calls: Rc::clone(&calls),
                responses,
            },
        )
        .get_with_policy(crate::ids::GroupRef::parse("did:example:group").unwrap())
        .unwrap();

        let group = result.group.as_ref().unwrap();
        assert_eq!(group.my_role.as_deref(), Some("owner"));
        assert_eq!(group.membership_status.as_deref(), Some("active"));
        assert_eq!(
            result.response_json().unwrap()["group_state_version"],
            "state-2"
        );
        assert_eq!(
            result.response_json().unwrap()["group_policy"]["message_security_profile"],
            "group-e2ee"
        );
        assert!(crate::groups::authoritative_group_e2ee_classification(
            "did:example:group",
            &result
        )
        .unwrap());

        let calls = calls.borrow();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].method, "group.get");
        assert!(calls[0].params["body"].get("include_policy").is_none());
        assert_eq!(calls[1].method, "group.get_info");
        assert_eq!(calls[1].endpoint, MESSAGE_RPC_ENDPOINT);
        assert_eq!(calls[1].params["meta"]["profile"], "anp.group.base.v1");
        assert!(calls[1].params["meta"].get("anp_version").is_none());
        assert_eq!(
            calls[1].params["meta"]["target"],
            json!({"kind":"group","did":"did:example:group"})
        );
        assert_eq!(calls[1].params["body"]["include_policy"], true);
        assert_eq!(calls[1].params["body"]["include_member_list"], false);
    }

    #[tokio::test]
    #[cfg(feature = "group-e2ee")]
    async fn get_with_policy_async_uses_the_same_authoritative_p4_merge() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let responses = Rc::new(RefCell::new(VecDeque::from([
            json!({
                "group_snapshot": {
                    "group_did": "did:example:group",
                    "my_role": "member",
                    "membership_status": "active",
                    "required_security_profile": "transport-protected"
                }
            }),
            json!({
                "group_did": "did:example:group",
                "group_state_version": "state-3",
                "group_profile": {"display_name": "Remote group"},
                "group_policy": {"message_security_profile": "group-e2ee"}
            }),
        ])));

        let result = GroupReadRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            SequencedRecordingTransport {
                calls: Rc::clone(&calls),
                responses,
            },
        )
        .get_with_policy_async(crate::ids::GroupRef::parse("did:example:group").unwrap())
        .await
        .unwrap();

        let group = result.group.as_ref().unwrap();
        assert_eq!(group.my_role.as_deref(), Some("member"));
        assert_eq!(group.membership_status.as_deref(), Some("active"));
        assert!(crate::groups::authoritative_group_e2ee_classification(
            "did:example:group",
            &result
        )
        .unwrap());
        let calls = calls.borrow();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].method, "group.get");
        assert!(calls[0].params["body"].get("include_policy").is_none());
        assert_eq!(calls[1].method, "group.get_info");
        assert_eq!(calls[1].params["meta"]["profile"], "anp.group.base.v1");
        assert!(calls[1].params["meta"].get("anp_version").is_none());
        assert_eq!(calls[1].params["body"]["include_policy"], true);
        assert_eq!(calls[1].params["body"]["include_member_list"], false);
    }

    #[test]
    fn authoritative_policy_merge_rejects_missing_p4_group_profile() {
        let error = merge_authoritative_group_policy(
            "did:example:group",
            json!({"group": {"group_did": "did:example:group"}}),
            json!({
                "group_did": "did:example:group",
                "group_state_version": "state-2",
                "group_policy": {"message_security_profile": "group-e2ee"}
            }),
        )
        .unwrap_err();

        assert!(matches!(error, crate::ImError::Serialization { .. }));
    }

    #[test]
    fn authoritative_policy_merge_rejects_non_object_p4_group_profile() {
        let error = merge_authoritative_group_policy(
            "did:example:group",
            json!({"group": {"group_did": "did:example:group"}}),
            json!({
                "group_did": "did:example:group",
                "group_state_version": "state-2",
                "group_profile": "malformed",
                "group_policy": {"message_security_profile": "group-e2ee"}
            }),
        )
        .unwrap_err();

        assert!(matches!(error, crate::ImError::Serialization { .. }));
    }

    #[derive(Clone)]
    struct ReadyGroupSessionProvider;

    impl SessionProvider for ReadyGroupSessionProvider {
        fn ensure_session(
            &self,
            scope: crate::auth::AuthScope,
        ) -> crate::ImResult<crate::auth::SessionBundle> {
            assert_eq!(scope, crate::auth::AuthScope::GroupMessaging);
            Ok(crate::auth::SessionBundle {
                subject: crate::ids::Did::parse("did:example:alice")?,
                scope,
                expires_at: None,
                refreshed: false,
                bearer_token: None,
            })
        }

        fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
            unreachable!("group read runtime should not refresh through the session provider")
        }

        fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("group read runtime should not read status")
        }
    }

    impl crate::internal::auth::session::AsyncSessionProvider for ReadyGroupSessionProvider {
        async fn ensure_session(
            &self,
            scope: crate::auth::AuthScope,
        ) -> crate::ImResult<crate::auth::SessionBundle> {
            assert_eq!(scope, crate::auth::AuthScope::GroupMessaging);
            Ok(crate::auth::SessionBundle {
                subject: crate::ids::Did::parse("did:example:alice")?,
                scope,
                expires_at: None,
                refreshed: false,
                bearer_token: None,
            })
        }

        async fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
            unreachable!("group read runtime should not refresh through the session provider")
        }

        async fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("group read runtime should not read status")
        }
    }

    struct RecordingTransport {
        calls: Rc<RefCell<Vec<RecordedCall>>>,
        response: Value,
    }

    impl AuthenticatedRpcTransport for RecordingTransport {
        fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            self.calls.borrow_mut().push(RecordedCall {
                endpoint: endpoint.to_string(),
                method: method.to_string(),
                params,
            });
            Ok(self.response.clone())
        }
    }

    impl crate::internal::transport::AsyncAuthenticatedRpcTransport for RecordingTransport {
        async fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            self.calls.borrow_mut().push(RecordedCall {
                endpoint: endpoint.to_string(),
                method: method.to_string(),
                params,
            });
            Ok(self.response.clone())
        }
    }

    struct SequencedRecordingTransport {
        calls: Rc<RefCell<Vec<RecordedCall>>>,
        responses: Rc<RefCell<VecDeque<Value>>>,
    }

    impl AuthenticatedRpcTransport for SequencedRecordingTransport {
        fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            self.calls.borrow_mut().push(RecordedCall {
                endpoint: endpoint.to_string(),
                method: method.to_string(),
                params,
            });
            Ok(self
                .responses
                .borrow_mut()
                .pop_front()
                .expect("recording transport response"))
        }
    }

    impl crate::internal::transport::AsyncAuthenticatedRpcTransport for SequencedRecordingTransport {
        async fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            self.calls.borrow_mut().push(RecordedCall {
                endpoint: endpoint.to_string(),
                method: method.to_string(),
                params,
            });
            Ok(self
                .responses
                .borrow_mut()
                .pop_front()
                .expect("recording transport response"))
        }
    }

    struct RecordedCall {
        endpoint: String,
        method: String,
        params: Value,
    }

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = unique_temp_root();
            let identities = root.join("identities");
            fs::create_dir_all(&identities).unwrap();
            fs::write(identities.join("default"), "alice\n").unwrap();
            fs::write(
                identities.join("registry.json"),
                r#"{
                  "default_identity": "alice",
                  "identities": [{
                    "id": "alice-id",
                    "did": "did:example:alice",
                    "local_alias": "alice",
                    "ready_for_auth": true,
                    "ready_for_messaging": true,
                    "missing": []
                  }]
                }"#,
            )
            .unwrap();
            fs::create_dir_all(identities.join("alice")).unwrap();
            Self { root }
        }

        fn client(&self) -> crate::core::ImClient {
            crate::core::ImCore::new(
                crate::ImCoreConfig {
                    service_base_url: crate::ServiceEndpoint::parse("https://example.test")
                        .unwrap(),
                    did_domain: "awiki.test".to_string(),
                    client_version_info: None,
                    user_service_endpoint: None,
                    message_service_endpoint: None,
                    mail_service_endpoint: None,
                    anp_service_endpoint: None,
                    anp_service_did: None,
                    ca_bundle: None,
                    transport_policy: crate::MessageTransportPolicy::HttpOnly,
                },
                crate::ImCorePaths {
                    identities: crate::paths::IdentityRegistryPaths {
                        identity_root_dir: self.root.join("identities"),
                        registry_path: self.root.join("identities").join("registry.json"),
                        default_identity_path: Some(self.root.join("identities").join("default")),
                    },
                    local_state: crate::paths::LocalStatePaths {
                        sqlite_path: self.root.join("local").join("im.sqlite"),
                    },
                    runtime: crate::paths::RuntimePaths {
                        cache_dir: self.root.join("cache"),
                        temp_dir: self.root.join("tmp"),
                    },
                },
            )
            .unwrap()
            .client(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_string(),
            ))
            .unwrap()
        }
    }

    fn unique_temp_root() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "im-core-group-read-runtime-{}-{nanos}",
            std::process::id()
        ))
    }
}
