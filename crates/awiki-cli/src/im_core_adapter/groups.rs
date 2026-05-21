use im_core::prelude::{
    AuthScope, Cursor, GroupListRequest, GroupMembersRequest, GroupMessagesRequest, GroupRef,
    PageLimit, SessionBundle,
};
use serde_json::{json, Value};

use crate::authsdk::Session;
use crate::config::Resolved;
use crate::identity::Manager;
use crate::message::{self, MessageError, WSProxyTransport};
use crate::runtime;
use crate::transportcfg::Profile;

pub fn get_group_via_im_core(
    resolved: &Resolved,
    manager: &Manager,
    client: &im_core::ImClient,
    identity_name: &str,
    group: String,
) -> Result<message::CommandResult, MessageError> {
    let record = message::require_active_identity(resolved, manager, identity_name)?;
    let group_ref = GroupRef::parse(&group).map_err(im_error_to_message_error)?;
    let bridge_result = im_core::compat::groups::get_group_with_bridge(
        client,
        GroupReadSessionProvider {
            subject: client.did().clone(),
            resolved,
            manager,
            record: record.clone(),
        },
        GroupReadLegacyTransport {
            resolved,
            manager,
            record: record.clone(),
        },
        im_core::compat::groups::GroupGetBridgeRequest { group: group_ref },
    )
    .map_err(im_error_to_message_error)?;
    let raw = bridge_result.raw;
    let mut warnings = group_control_warnings(resolved, bridge_result.warnings);
    warnings.extend(message::persist_group_snapshot(resolved, &record, &raw));
    let snapshot = message::cached_group_snapshot(resolved, &record, &group)
        .or_else(|| message::normalize_group_snapshot(&raw))
        .unwrap_or(Value::Null);
    Ok(message::CommandResult {
        data: json!({
            "group": snapshot,
            "source": message::group_control_source(&raw),
        }),
        summary: "Loaded group snapshot".to_string(),
        warnings: message::compact_warnings(warnings),
    })
}

pub fn list_groups_via_im_core(
    resolved: &Resolved,
    manager: &Manager,
    client: &im_core::ImClient,
    identity_name: &str,
    limit: i64,
) -> Result<message::CommandResult, MessageError> {
    let record = message::require_active_identity(resolved, manager, identity_name)?;
    let request = GroupListRequest {
        limit: page_limit(limit, 50)?,
    };
    let bridge_result = im_core::compat::groups::list_groups_with_bridge(
        client,
        GroupReadSessionProvider {
            subject: client.did().clone(),
            resolved,
            manager,
            record: record.clone(),
        },
        GroupReadLegacyTransport {
            resolved,
            manager,
            record,
        },
        im_core::compat::groups::GroupListBridgeRequest { request },
    )
    .map_err(im_error_to_message_error)?;
    let raw = bridge_result.raw;
    let groups = message::values_from_array(raw.get("groups"));
    let total = message::int_value(raw.get("total"), groups.len() as i64);
    Ok(message::CommandResult {
        data: json!({
            "groups": groups,
            "total": total,
            "source": message::group_control_source(&raw),
        }),
        summary: format!("Loaded {total} groups"),
        warnings: group_control_warnings(resolved, bridge_result.warnings),
    })
}

pub fn group_members_via_im_core(
    resolved: &Resolved,
    manager: &Manager,
    client: &im_core::ImClient,
    identity_name: &str,
    group: String,
    limit: i64,
) -> Result<message::CommandResult, MessageError> {
    let record = message::require_active_identity(resolved, manager, identity_name)?;
    let request = GroupMembersRequest {
        group: GroupRef::parse(&group).map_err(im_error_to_message_error)?,
        limit: page_limit(limit, 100)?,
    };
    let bridge_result = im_core::compat::groups::list_group_members_with_bridge(
        client,
        GroupReadSessionProvider {
            subject: client.did().clone(),
            resolved,
            manager,
            record: record.clone(),
        },
        GroupReadLegacyTransport {
            resolved,
            manager,
            record: record.clone(),
        },
        im_core::compat::groups::GroupMembersBridgeRequest { request },
    )
    .map_err(im_error_to_message_error)?;
    let raw = bridge_result.raw;
    let mut warnings = group_control_warnings(resolved, bridge_result.warnings);
    warnings.extend(message::persist_group_members(
        resolved, &record, &group, &raw,
    ));
    let members = message::cached_group_members(resolved, &record, &group, limit)
        .filter(|items| !items.is_empty())
        .unwrap_or_else(|| message::values_from_array(raw.get("members")));
    let total = message::int_value(raw.get("total"), members.len() as i64);
    Ok(message::CommandResult {
        data: json!({
            "group": group,
            "members": members,
            "total": total,
            "source": message::group_control_source(&raw),
        }),
        summary: format!("Loaded {total} group members"),
        warnings: message::compact_warnings(warnings),
    })
}

pub fn group_messages_via_im_core(
    resolved: &Resolved,
    manager: &Manager,
    client: &im_core::ImClient,
    identity_name: &str,
    group: String,
    limit: i64,
    cursor: String,
) -> Result<message::CommandResult, MessageError> {
    let record = message::require_active_identity(resolved, manager, identity_name)?;
    let source_mode = message::runtime_mode(resolved);
    let request = GroupMessagesRequest {
        group: GroupRef::parse(&group).map_err(im_error_to_message_error)?,
        limit: page_limit(limit, 50)?,
        cursor: optional_cursor(&cursor)?,
    };
    let (mut raw, mut warnings, result_source_mode) = if source_mode
        == runtime::bridge::MODE_WEBSOCKET
    {
        match group_messages_websocket(resolved, manager, client, &record, request.clone())? {
            GroupMessagesOutcome::Remote {
                raw,
                warnings,
                source_mode,
            } => (raw, warnings, source_mode),
            GroupMessagesOutcome::LocalCache(result) => return Ok(result),
        }
    } else {
        let bridge_result = list_group_messages_http(resolved, manager, client, &record, request)?;
        (bridge_result.raw, bridge_result.warnings, source_mode)
    };

    warnings.extend(message::maybe_decrypt_group_messages(
        resolved, &record, &group, &mut raw,
    ));
    warnings.extend(message::persist_group_messages(
        resolved, &record, &group, &raw,
    ));
    let messages = message::cached_group_messages(resolved, &record, &group, limit, &cursor)
        .filter(|items| !items.is_empty())
        .unwrap_or_else(|| message::values_from_array(raw.get("messages")));
    let total = message::int_value(raw.get("total"), messages.len() as i64);
    Ok(message::CommandResult {
        data: json!({
            "group": group,
            "messages": messages,
            "total": total,
            "has_more": message::bool_value(raw.get("has_more")),
            "next_since_seq": raw.get("next_since_seq").cloned().unwrap_or(Value::Null),
            "source": source_with_default_for_mode(&raw, result_source_mode),
        }),
        summary: format!("Loaded {total} group messages"),
        warnings: message::compact_warnings(warnings),
    })
}

enum GroupMessagesOutcome {
    Remote {
        raw: Value,
        warnings: Vec<String>,
        source_mode: &'static str,
    },
    LocalCache(message::CommandResult),
}

fn group_messages_websocket(
    resolved: &Resolved,
    manager: &Manager,
    client: &im_core::ImClient,
    record: &crate::identity::types::StoredIdentity,
    request: GroupMessagesRequest,
) -> Result<GroupMessagesOutcome, MessageError> {
    let legacy_request = message::GroupMessagesRequest {
        identity_name: record.identity_name.clone(),
        group: request.group.as_str().to_string(),
        limit: i64::from(request.limit.0),
        cursor: request
            .cursor
            .as_ref()
            .map(|cursor| cursor.as_str().to_string())
            .unwrap_or_default(),
        skip: 0,
    };
    let bridge = WSProxyTransport::new(resolved, &record.identity_name);
    match bridge.list_group_messages(legacy_request.clone()) {
        Ok(result) => Ok(GroupMessagesOutcome::Remote {
            raw: Value::Object(result),
            warnings: Vec::new(),
            source_mode: runtime::bridge::MODE_WEBSOCKET,
        }),
        Err(bridge_err) => {
            if let Some(cached) = message::cached_group_messages(
                resolved,
                record,
                &legacy_request.group,
                legacy_request.limit,
                &legacy_request.cursor,
            )
            .filter(|items| !items.is_empty())
            {
                return Ok(GroupMessagesOutcome::LocalCache(
                    group_messages_local_cache_result(&legacy_request, cached, &bridge_err),
                ));
            }
            match list_group_messages_http(resolved, manager, client, record, request) {
                Ok(result) => {
                    crate::traceutil::mark_fallback(
                        "websocket_to_http",
                        Some(&bridge_err.to_string()),
                    );
                    Ok(GroupMessagesOutcome::Remote {
                        raw: result.raw,
                        warnings: vec![message::websocket_http_fallback_warning(Some(&bridge_err))],
                        source_mode: runtime::bridge::MODE_HTTP,
                    })
                }
                Err(_) => Err(bridge_err),
            }
        }
    }
}

fn list_group_messages_http(
    resolved: &Resolved,
    manager: &Manager,
    client: &im_core::ImClient,
    record: &crate::identity::types::StoredIdentity,
    request: GroupMessagesRequest,
) -> Result<im_core::groups::GroupReadResult, MessageError> {
    im_core::compat::groups::list_group_messages_with_bridge(
        client,
        GroupReadSessionProvider {
            subject: client.did().clone(),
            resolved,
            manager,
            record: record.clone(),
        },
        GroupReadLegacyTransport {
            resolved,
            manager,
            record: record.clone(),
        },
        im_core::compat::groups::GroupMessagesBridgeRequest { request },
    )
    .map_err(im_error_to_message_error)
}

fn group_messages_local_cache_result(
    request: &message::GroupMessagesRequest,
    messages: Vec<Value>,
    bridge_err: &MessageError,
) -> message::CommandResult {
    let total = messages.len();
    message::CommandResult {
        data: json!({
            "group": request.group,
            "messages": messages,
            "total": total,
            "source": "local_ws_cache_fallback",
        }),
        summary: "Loaded group messages from local cache".to_string(),
        warnings: vec![message::websocket_cache_fallback_warning(Some(bridge_err))],
    }
}

struct GroupReadSessionProvider<'a> {
    subject: im_core::prelude::Did,
    resolved: &'a Resolved,
    manager: &'a Manager,
    record: crate::identity::types::StoredIdentity,
}

impl im_core::compat::groups::BridgeGroupSessionProvider for GroupReadSessionProvider<'_> {
    fn ensure_group_messaging_session(&self) -> im_core::ImResult<SessionBundle> {
        let session = message::auth_session(self.resolved, self.manager, &self.record)
            .map_err(message_error_to_im_error)?;
        Ok(SessionBundle {
            subject: self.subject.clone(),
            scope: AuthScope::GroupMessaging,
            expires_at: None,
            refreshed: session.current_jwt().trim() != self.record.jwt_token.trim(),
        })
    }
}

struct GroupReadLegacyTransport<'a> {
    resolved: &'a Resolved,
    manager: &'a Manager,
    record: crate::identity::types::StoredIdentity,
}

impl im_core::compat::groups::BridgeAuthenticatedRpcTransport for GroupReadLegacyTransport<'_> {
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> im_core::ImResult<Value> {
        send_authenticated_group_read_rpc_with_fallback(
            self.resolved,
            self.manager,
            &self.record,
            endpoint,
            method,
            params,
        )
        .map_err(message_error_to_im_error)
    }
}

fn send_authenticated_group_read_rpc_with_fallback(
    resolved: &Resolved,
    manager: &Manager,
    record: &crate::identity::types::StoredIdentity,
    endpoint: &str,
    method: &str,
    params: Value,
) -> Result<Value, MessageError> {
    match send_authenticated_group_read_rpc(
        resolved,
        manager,
        record,
        endpoint,
        method,
        params.clone(),
    ) {
        Ok(result) => Ok(result),
        Err(err) if message::is_session_unauthorized(&err) => {
            let refreshed = message::refresh_jwt_fallback(resolved, manager, record).ok();
            match send_authenticated_group_read_rpc(
                resolved,
                manager,
                refreshed.as_ref().unwrap_or(record),
                endpoint,
                method,
                params,
            ) {
                Ok(result) => Ok(result),
                Err(_) => Err(err),
            }
        }
        Err(err) => Err(err),
    }
}

fn send_authenticated_group_read_rpc(
    resolved: &Resolved,
    manager: &Manager,
    record: &crate::identity::types::StoredIdentity,
    endpoint: &str,
    method: &str,
    params: Value,
) -> Result<Value, MessageError> {
    let mut auth = auth_session(resolved, manager, record)?;
    let client = message::Client::new(resolved)?;
    client.authenticated_rpc_call_profile(
        Profile::RpcReadHeavy,
        endpoint,
        method,
        params,
        &mut auth,
    )
}

fn auth_session(
    resolved: &Resolved,
    manager: &Manager,
    record: &crate::identity::types::StoredIdentity,
) -> Result<Session, MessageError> {
    message::auth_session(resolved, manager, record)
}

fn group_control_warnings(resolved: &Resolved, mut warnings: Vec<String>) -> Vec<String> {
    warnings.extend(message::group_control_warnings(resolved));
    message::compact_warnings(warnings)
}

fn source_with_default_for_mode(raw: &Value, mode: &str) -> String {
    raw.get("source")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(if mode == runtime::bridge::MODE_WEBSOCKET {
            "local_ws_cache"
        } else {
            "remote_http"
        })
        .to_string()
}

fn page_limit(value: i64, fallback: u32) -> Result<PageLimit, MessageError> {
    let value = if value <= 0 {
        fallback
    } else {
        u32::try_from(value).map_err(|_| MessageError::Json("limit is too large".to_string()))?
    };
    PageLimit::new(value).map_err(im_error_to_message_error)
}

fn optional_cursor(value: &str) -> Result<Option<Cursor>, MessageError> {
    if value.trim().is_empty() {
        return Ok(None);
    }
    Cursor::parse(value)
        .map(Some)
        .map_err(im_error_to_message_error)
}

fn im_error_to_message_error(err: im_core::ImError) -> MessageError {
    match err {
        im_core::ImError::InvalidInput { field, .. } if field.as_deref() == Some("group") => {
            MessageError::GroupRequired
        }
        im_core::ImError::GroupNotFound { .. } => MessageError::GroupRequired,
        im_core::ImError::AuthRequired | im_core::ImError::SessionExpired => {
            MessageError::IdentityRequired("authentication is required".to_string())
        }
        im_core::ImError::IdentityNotReady { identity, missing } => MessageError::IdentityRequired(
            format!("identity {identity} is not ready: {}", missing.join(", ")),
        ),
        im_core::ImError::Service {
            status_code,
            code,
            message,
        } => MessageError::Service(crate::identity::wire::ServiceError {
            status_code: status_code.unwrap_or_default(),
            rpc_code: code
                .and_then(|value| value.parse().ok())
                .unwrap_or_default(),
            message,
            data: None,
        }),
        im_core::ImError::TransportUnavailable { detail } => {
            MessageError::TransportUnavailable(detail)
        }
        err => MessageError::Internal(err.to_string()),
    }
}

fn message_error_to_im_error(err: MessageError) -> im_core::ImError {
    match err {
        MessageError::Service(service_err) => im_core::ImError::Service {
            status_code: (service_err.status_code != 0).then_some(service_err.status_code),
            code: (service_err.rpc_code != 0).then(|| service_err.rpc_code.to_string()),
            message: service_err.message,
        },
        MessageError::TransportUnavailable(detail) => {
            im_core::ImError::TransportUnavailable { detail }
        }
        MessageError::GroupRequired => {
            im_core::ImError::invalid_input(Some("group".to_string()), "group target is required")
        }
        MessageError::IdentityRequired(message) => im_core::ImError::IdentityNotReady {
            identity: message,
            missing: Vec::new(),
        },
        err => im_core::ImError::Internal {
            message: err.to_string(),
        },
    }
}
