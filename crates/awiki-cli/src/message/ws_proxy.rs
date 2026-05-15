use crate::config::Resolved;
use crate::message::service::string_value;
use crate::message::types::{
    GroupCreateRequest, GroupGetRequest, GroupInfoRequest, GroupJoinRequest, GroupLeaveRequest,
    GroupListRequest, GroupMemberRequest, GroupMembersRequest, GroupMessagesRequest,
    HistoryRequest, InboxRequest, MarkReadRequest, MessageError, SendRequest,
};
use crate::runtime::bridge::{self, BridgeRequest};
use serde_json::{json, Map, Value};

#[derive(Debug, Clone)]
pub struct WSProxyTransport<'a> {
    resolved: &'a Resolved,
    identity_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct DirectSendResult {
    #[serde(default)]
    pub accepted: bool,
    #[serde(default)]
    pub message_id: String,
    #[serde(default)]
    pub operation_id: String,
    #[serde(default)]
    pub target_did: String,
    #[serde(default)]
    pub accepted_at: String,
    #[serde(default)]
    pub final_acceptance: bool,
    #[serde(default)]
    pub delivery_state: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct GroupSendResult {
    #[serde(default)]
    pub accepted: bool,
    #[serde(default)]
    pub final_acceptance: bool,
    #[serde(default)]
    pub group_did: String,
    #[serde(default)]
    pub message_id: String,
    #[serde(default)]
    pub operation_id: String,
    #[serde(default)]
    pub group_event_seq: String,
    #[serde(default)]
    pub group_state_version: String,
    #[serde(default)]
    pub accepted_at: String,
}

impl<'a> WSProxyTransport<'a> {
    pub fn new(resolved: &'a Resolved, identity_name: &str) -> Self {
        Self {
            resolved,
            identity_name: identity_name.to_string(),
        }
    }

    pub fn send_direct(&self, request: SendRequest) -> Result<DirectSendResult, MessageError> {
        let result = self.call(
            "direct.send",
            json!({
                "target": request.target,
                "text": request.text,
                "type": request.message_type,
            }),
        )?;
        Ok(DirectSendResult::from_bridge_result(&result))
    }

    pub fn send_group(&self, request: SendRequest) -> Result<GroupSendResult, MessageError> {
        let result = self.call(
            "group.send",
            json!({
                "group": request.group,
                "text": request.text,
                "type": request.message_type,
            }),
        )?;
        Ok(GroupSendResult::from_bridge_result(&result))
    }

    pub fn get_inbox(&self, request: InboxRequest) -> Result<Map<String, Value>, MessageError> {
        self.call(
            "inbox.get",
            json!({
                "with": request.with,
                "limit": request.limit,
                "scope": request.scope,
                "mark_read": request.mark_read,
                "unread": request.unread_only,
            }),
        )
    }

    pub fn get_history(&self, request: HistoryRequest) -> Result<Map<String, Value>, MessageError> {
        let mut params = Map::new();
        params.insert("with".to_string(), Value::String(request.with));
        params.insert("limit".to_string(), json!(request.limit));
        params.insert("cursor".to_string(), Value::String(request.cursor));
        if request.skip > 0 {
            params.insert("skip".to_string(), json!(request.skip));
        }
        self.call("direct.get_history", Value::Object(params))
    }

    pub fn mark_read(&self, request: MarkReadRequest) -> Result<Map<String, Value>, MessageError> {
        self.call(
            "inbox.mark_read",
            json!({ "message_ids": request.message_ids }),
        )
    }

    pub fn create_group(
        &self,
        request: GroupCreateRequest,
    ) -> Result<Map<String, Value>, MessageError> {
        self.call(
            "group.create",
            json!({
                "name": request.name,
                "description": request.description,
                "discoverability": request.discoverability,
                "admission_mode": request.admission_mode,
                "slug": request.slug,
                "goal": request.goal,
                "rules": request.rules,
                "message_prompt": request.message_prompt,
                "doc_url": request.doc_url,
                "attachments_allowed": request.attachments_allowed,
                "max_members": request.max_members,
                "member_max_messages": request.member_max_messages,
                "member_max_total_chars": request.member_max_total_chars,
            }),
        )
    }

    pub fn get_group_info(
        &self,
        request: GroupInfoRequest,
    ) -> Result<Map<String, Value>, MessageError> {
        self.call(
            "group.get_info",
            json!({
                "group": request.group,
                "include_policy": request.include_policy,
                "include_member_list": request.include_member_list,
            }),
        )
    }

    pub fn join_group(
        &self,
        request: GroupJoinRequest,
    ) -> Result<Map<String, Value>, MessageError> {
        self.call(
            "group.join",
            json!({ "group": request.group, "reason_text": request.reason_text }),
        )
    }

    pub fn add_group_member(
        &self,
        request: GroupMemberRequest,
    ) -> Result<Map<String, Value>, MessageError> {
        self.call(
            "group.add",
            json!({
                "group": request.group,
                "member": request.member,
                "role": request.role,
                "reason_text": request.reason_text,
            }),
        )
    }

    pub fn remove_group_member(
        &self,
        request: GroupMemberRequest,
    ) -> Result<Map<String, Value>, MessageError> {
        self.call(
            "group.remove",
            json!({
                "group": request.group,
                "member": request.member,
                "reason_text": request.reason_text,
            }),
        )
    }

    pub fn leave_group(
        &self,
        request: GroupLeaveRequest,
    ) -> Result<Map<String, Value>, MessageError> {
        self.call("group.leave", json!({ "group": request.group }))
    }

    pub fn get_group(&self, request: GroupGetRequest) -> Result<Map<String, Value>, MessageError> {
        self.call("group.get", json!({ "group": request.group }))
    }

    pub fn list_groups(
        &self,
        request: GroupListRequest,
    ) -> Result<Map<String, Value>, MessageError> {
        self.call("group.list", json!({ "limit": request.limit }))
    }

    pub fn list_group_members(
        &self,
        request: GroupMembersRequest,
    ) -> Result<Map<String, Value>, MessageError> {
        self.call(
            "group.list_members",
            json!({ "group": request.group, "limit": request.limit }),
        )
    }

    pub fn list_group_messages(
        &self,
        request: GroupMessagesRequest,
    ) -> Result<Map<String, Value>, MessageError> {
        let mut params = Map::new();
        params.insert("group".to_string(), Value::String(request.group));
        params.insert("limit".to_string(), json!(request.limit));
        params.insert("cursor".to_string(), Value::String(request.cursor));
        if request.skip > 0 {
            params.insert("skip".to_string(), json!(request.skip));
        }
        self.call("group.list_messages", Value::Object(params))
    }

    pub fn update_group_profile(
        &self,
        request: GroupGetRequest,
        patch: Map<String, Value>,
    ) -> Result<Map<String, Value>, MessageError> {
        self.call(
            "group.update_profile",
            json!({ "group": request.group, "patch": patch }),
        )
    }

    pub fn update_group_policy(
        &self,
        request: GroupGetRequest,
        patch: Map<String, Value>,
    ) -> Result<Map<String, Value>, MessageError> {
        self.call(
            "group.update_policy",
            json!({ "group": request.group, "patch": patch }),
        )
    }

    fn call(&self, method: &str, params: Value) -> Result<Map<String, Value>, MessageError> {
        let params = value_object(params);
        bridge::call_local_bridge(
            BridgeRequest {
                method: method.to_string(),
                params,
                identity_name: self.identity_name.clone(),
            },
            self.resolved,
        )
        .map_err(|err| MessageError::transport_unavailable(err.to_string()))
    }
}

fn value_object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

impl DirectSendResult {
    fn from_bridge_result(result: &Map<String, Value>) -> Self {
        Self {
            accepted: bool_value(result.get("accepted")),
            message_id: string_value(result.get("message_id")),
            operation_id: string_value(result.get("operation_id")),
            target_did: string_value(result.get("target_did")),
            accepted_at: string_value(result.get("accepted_at")),
            final_acceptance: bool_value(result.get("final_acceptance")),
            delivery_state: string_value(result.get("delivery_state")),
        }
    }
}

impl GroupSendResult {
    fn from_bridge_result(result: &Map<String, Value>) -> Self {
        Self {
            accepted: bool_value(result.get("accepted")),
            final_acceptance: bool_value(result.get("final_acceptance")),
            group_did: string_value(result.get("group_did")),
            message_id: string_value(result.get("message_id")),
            operation_id: string_value(result.get("operation_id")),
            group_event_seq: string_value(result.get("group_event_seq")),
            group_state_version: string_value(result.get("group_state_version")),
            accepted_at: string_value(result.get("accepted_at")),
        }
    }
}

fn bool_value(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(value)) => *value,
        _ => false,
    }
}
