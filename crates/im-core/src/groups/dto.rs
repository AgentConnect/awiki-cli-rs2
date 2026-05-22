use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupReadResult {
    pub group: Option<GroupSnapshot>,
    pub groups: Vec<GroupSummary>,
    pub members: Vec<GroupMember>,
    pub messages: crate::ids::Page<crate::messages::Message>,
    pub total: Option<u32>,
    pub source: Option<String>,
    #[serde(skip)]
    diagnostic_raw: Option<Value>,
    pub warnings: Vec<String>,
}

impl GroupReadResult {
    pub(crate) fn from_diagnostic_raw(raw: Value, warnings: Vec<String>) -> Self {
        let group = group_snapshot_from_value(raw.get("group").unwrap_or(&raw));
        let groups = values_from_array(raw.get("groups"))
            .into_iter()
            .filter_map(group_summary_from_value)
            .collect();
        let members = values_from_array(raw.get("members"))
            .into_iter()
            .filter_map(group_member_from_value)
            .collect();
        let message_items = values_from_array(raw.get("messages"))
            .into_iter()
            .filter_map(group_message_from_value)
            .collect::<Vec<_>>();
        let messages = crate::ids::Page {
            items: message_items,
            next_cursor: cursor_from_value(
                raw.get("next_cursor").or_else(|| raw.get("next_since_seq")),
            ),
            has_more: bool_value(raw.get("has_more")),
        };
        Self {
            group,
            groups,
            members,
            messages,
            total: u32_value(raw.get("total")),
            source: optional_string(raw.get("source")),
            diagnostic_raw: Some(raw),
            warnings,
        }
    }

    pub fn diagnostic_raw(&self) -> Option<&Value> {
        self.diagnostic_raw.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupCreateRequest {
    pub name: String,
    pub description: Option<String>,
    pub discoverability: Option<String>,
    pub admission_mode: Option<String>,
    pub message_security_profile: Option<String>,
    pub e2ee: bool,
    pub slug: Option<String>,
    pub goal: Option<String>,
    pub rules: Option<String>,
    pub message_prompt: Option<String>,
    pub doc_url: Option<String>,
    pub attachments_allowed: Option<bool>,
    pub max_members: Option<String>,
    pub member_max_messages: Option<i64>,
    pub member_max_total_chars: Option<i64>,
    pub service_did: crate::ids::Did,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupJoinRequest {
    pub group: crate::ids::GroupRef,
    pub reason_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupLeaveRequest {
    pub group: crate::ids::GroupRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupMemberMutationRequest {
    pub group: crate::ids::GroupRef,
    pub member: crate::ids::Did,
    pub role: Option<String>,
    pub reason_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GroupProfilePatch {
    pub name: Option<String>,
    pub description: Option<String>,
    pub discoverability: Option<String>,
    pub slug: Option<String>,
    pub goal: Option<String>,
    pub rules: Option<String>,
    pub message_prompt: Option<String>,
    pub doc_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GroupPolicyPatch {
    pub admission_mode: Option<String>,
    pub attachments_allowed: Option<bool>,
    pub max_members: Option<String>,
    pub member_max_messages: Option<i64>,
    pub member_max_total_chars: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupUpdateProfileRequest {
    pub group: crate::ids::GroupRef,
    pub patch: GroupProfilePatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupUpdatePolicyRequest {
    pub group: crate::ids::GroupRef,
    pub patch: GroupPolicyPatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupListRequest {
    pub limit: crate::ids::PageLimit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupMembersRequest {
    pub group: crate::ids::GroupRef,
    pub limit: crate::ids::PageLimit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupMessagesRequest {
    pub group: crate::ids::GroupRef,
    pub limit: crate::ids::PageLimit,
    pub cursor: Option<crate::ids::Cursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupSnapshot {
    pub id: Option<String>,
    pub did: crate::ids::GroupRef,
    pub name: Option<String>,
    pub description: Option<String>,
    pub my_role: Option<String>,
    pub membership_status: Option<String>,
    pub member_count: Option<u32>,
    pub last_message_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupSummary {
    pub id: Option<String>,
    pub did: crate::ids::GroupRef,
    pub name: Option<String>,
    pub membership_status: Option<String>,
    pub member_count: Option<u32>,
    pub last_message_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupMember {
    pub did: Option<crate::ids::Did>,
    pub handle: Option<crate::ids::Handle>,
    pub role: Option<String>,
    pub status: Option<String>,
    pub joined_at: Option<String>,
}

fn group_snapshot_from_value(value: &Value) -> Option<GroupSnapshot> {
    let object = value.as_object()?;
    let did = group_ref_from_object(object)?;
    Some(GroupSnapshot {
        id: optional_string(object.get("id")).or_else(|| optional_string(object.get("group_id"))),
        did,
        name: optional_string(object.get("name"))
            .or_else(|| optional_string(object.get("display_name")))
            .or_else(|| nested_string(object.get("group_profile"), "display_name")),
        description: optional_string(object.get("description"))
            .or_else(|| nested_string(object.get("group_profile"), "description")),
        my_role: optional_string(object.get("my_role"))
            .or_else(|| optional_string(object.get("member_role")))
            .or_else(|| optional_string(object.get("actor_membership_role"))),
        membership_status: optional_string(object.get("membership_status"))
            .or_else(|| optional_string(object.get("member_status")))
            .or_else(|| optional_string(object.get("actor_membership_status")))
            .or_else(|| optional_string(object.get("status"))),
        member_count: u32_value(object.get("member_count")),
        last_message_at: optional_string(object.get("last_message_at")),
    })
}

fn group_summary_from_value(value: Value) -> Option<GroupSummary> {
    let object = value.as_object()?;
    let did = group_ref_from_object(object)?;
    Some(GroupSummary {
        id: optional_string(object.get("id")).or_else(|| optional_string(object.get("group_id"))),
        did,
        name: optional_string(object.get("name"))
            .or_else(|| optional_string(object.get("display_name")))
            .or_else(|| nested_string(object.get("group_profile"), "display_name")),
        membership_status: optional_string(object.get("membership_status"))
            .or_else(|| optional_string(object.get("member_status")))
            .or_else(|| optional_string(object.get("status"))),
        member_count: u32_value(object.get("member_count")),
        last_message_at: optional_string(object.get("last_message_at")),
    })
}

fn group_member_from_value(value: Value) -> Option<GroupMember> {
    let object = value.as_object()?;
    Some(GroupMember {
        did: optional_string(object.get("did"))
            .or_else(|| optional_string(object.get("member_did")))
            .or_else(|| optional_string(object.get("agent_did")))
            .and_then(|value| crate::ids::Did::parse(value).ok()),
        handle: optional_string(object.get("handle"))
            .or_else(|| optional_string(object.get("member_handle")))
            .or_else(|| optional_string(object.get("agent_handle")))
            .and_then(|value| crate::ids::Handle::parse(value, "").ok()),
        role: optional_string(object.get("role")),
        status: optional_string(object.get("status")),
        joined_at: optional_string(object.get("joined_at")),
    })
}

fn group_message_from_value(value: Value) -> Option<crate::messages::Message> {
    let object = value.as_object()?;
    let id = optional_string(object.get("id"))
        .or_else(|| optional_string(object.get("message_id")))
        .or_else(|| optional_string(object.get("msg_id")))?;
    let group_did = optional_string(object.get("group_did"))
        .or_else(|| optional_string(object.get("group")))
        .unwrap_or_else(|| "group:unknown".to_string());
    let sender = optional_string(object.get("sender_did"))
        .unwrap_or_else(|| "did:unknown:sender".to_string());
    let content_type = optional_string(object.get("content_type"));
    let body = if let Some(text) = optional_string(object.get("text"))
        .or_else(|| optional_string(object.get("content")))
        .or_else(|| nested_string(object.get("body"), "text"))
    {
        crate::messages::MessageBodyView::Text {
            text,
            kind: message_kind(content_type.as_deref()),
        }
    } else {
        crate::messages::MessageBodyView::Unsupported {
            content_type: content_type.clone(),
        }
    };
    let group = crate::ids::GroupRef::parse(&group_did).ok()?;
    Some(crate::messages::Message {
        id: crate::ids::MessageId::parse(id).ok()?,
        thread: crate::messages::ThreadRef::Group(group.clone()),
        direction: crate::messages::MessageDirection::Unknown,
        sender: crate::ids::PeerRef::parse(sender, "").ok()?,
        receiver: None,
        group: Some(group),
        body,
        sent_at: optional_string(object.get("sent_at"))
            .or_else(|| optional_string(object.get("created_at"))),
        received_at: optional_string(object.get("received_at")),
        metadata: crate::messages::MessageMetadata {
            operation_id: optional_string(object.get("operation_id")),
            delivery_state: optional_string(object.get("delivery_state")),
            server_sequence: i64_value(object.get("server_seq"))
                .or_else(|| i64_value(object.get("sequence")))
                .or_else(|| i64_value(object.get("group_event_seq"))),
            content_type,
            ..crate::messages::MessageMetadata::default()
        },
    })
}

fn group_ref_from_object(object: &serde_json::Map<String, Value>) -> Option<crate::ids::GroupRef> {
    optional_string(object.get("group_did"))
        .or_else(|| optional_string(object.get("did")))
        .or_else(|| optional_string(object.get("id")))
        .and_then(|value| crate::ids::GroupRef::parse(value).ok())
}

fn values_from_array(value: Option<&Value>) -> Vec<Value> {
    value
        .and_then(Value::as_array)
        .map(|items| items.to_vec())
        .unwrap_or_default()
}

fn optional_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn nested_string(value: Option<&Value>, key: &str) -> Option<String> {
    value
        .and_then(Value::as_object)
        .and_then(|object| optional_string(object.get(key)))
}

fn cursor_from_value(value: Option<&Value>) -> Option<crate::ids::Cursor> {
    optional_string(value).and_then(|value| crate::ids::Cursor::parse(value).ok())
}

fn u32_value(value: Option<&Value>) -> Option<u32> {
    value
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn i64_value(value: Option<&Value>) -> Option<i64> {
    value.and_then(Value::as_i64)
}

fn bool_value(value: Option<&Value>) -> bool {
    value.and_then(Value::as_bool).unwrap_or(false)
}

fn message_kind(content_type: Option<&str>) -> crate::messages::MessageKind {
    match content_type.map(str::trim) {
        Some("text/markdown" | "markdown" | "text/x-markdown") => {
            crate::messages::MessageKind::Markdown
        }
        _ => crate::messages::MessageKind::Text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn group_result_projects_domain_fields_and_keeps_raw_diagnostic() {
        let result = GroupReadResult::from_diagnostic_raw(
            json!({
                "group_did": "did:example:group",
                "name": "Demo",
                "membership_status": "active",
                "member_count": 2,
                "groups": [{
                    "group_did": "did:example:group",
                    "name": "Demo",
                    "membership_status": "active"
                }],
                "members": [{
                    "member_did": "did:example:bob",
                    "handle": "bob.example",
                    "role": "member",
                    "status": "active"
                }],
                "messages": [{
                    "id": "msg-1",
                    "group_did": "did:example:group",
                    "sender_did": "did:example:bob",
                    "text": "hello",
                    "sent_at": "2026-01-01T00:00:00Z"
                }],
                "total": 1,
                "has_more": false,
                "source": "remote_http"
            }),
            vec!["normalized".to_string()],
        );

        let group = result.group.as_ref().expect("group snapshot");
        assert_eq!(group.did.as_str(), "did:example:group");
        assert_eq!(group.name.as_deref(), Some("Demo"));
        assert_eq!(group.member_count, Some(2));
        assert_eq!(result.groups[0].did.as_str(), "did:example:group");
        assert_eq!(
            result.members[0].did.as_ref().map(crate::ids::Did::as_str),
            Some("did:example:bob")
        );
        assert_eq!(result.messages.items[0].id.as_str(), "msg-1");
        assert_eq!(result.total, Some(1));
        assert_eq!(result.source.as_deref(), Some("remote_http"));
        assert_eq!(result.warnings, vec!["normalized"]);
        assert_eq!(
            result.diagnostic_raw().and_then(|raw| raw.get("group_did")),
            Some(&json!("did:example:group"))
        );
    }
}
