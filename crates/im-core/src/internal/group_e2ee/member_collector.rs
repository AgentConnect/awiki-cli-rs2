use std::collections::BTreeSet;

use serde_json::Value;

const PAGE_LIMIT: u16 = 100;
const IMPLEMENTATION_HARD_CAP: usize = 1_000;
const MAX_PAGES: usize = 10;
const MAX_ATTEMPTS: usize = 4;
const DEFAULT_PRODUCT_MAX_MEMBERS: usize = 500;
const PRODUCT_MAX_MEMBERS_CAP: usize = 500;

#[derive(Debug, Clone)]
pub(crate) struct CompleteGroupMembers {
    pub(crate) members: Vec<crate::groups::GroupMember>,
    pub(crate) group_state_version: String,
    pub(crate) total: u32,
}

pub(crate) fn product_max_members(authoritative_group: &Value) -> crate::ImResult<usize> {
    let value = authoritative_group
        .get("group_policy")
        .and_then(Value::as_object)
        .and_then(|policy| policy.get("max_members"));
    let parsed = match value {
        None => DEFAULT_PRODUCT_MAX_MEMBERS,
        Some(Value::String(value)) => parse_canonical_positive(value)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(crate::ImError::InventoryTooLarge)?,
        Some(Value::Number(value)) => value
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .filter(|value| *value > 0)
            .ok_or(crate::ImError::InventoryTooLarge)?,
        Some(_) => return Err(crate::ImError::InventoryTooLarge),
    };
    if parsed > PRODUCT_MAX_MEMBERS_CAP {
        return Err(crate::ImError::InventoryTooLarge);
    }
    Ok(parsed)
}

pub(crate) fn collect_complete_group_members(
    client: &crate::core::ImClient,
    group: crate::ids::GroupRef,
    expected_version: Option<&str>,
    product_max_members: usize,
) -> crate::ImResult<CompleteGroupMembers> {
    for attempt in 0..MAX_ATTEMPTS {
        match collect_attempt(client, group.clone(), expected_version, product_max_members) {
            Err(crate::ImError::CursorStale)
                if expected_version.is_none() && attempt + 1 < MAX_ATTEMPTS =>
            {
                continue
            }
            result => return result,
        }
    }
    Err(crate::ImError::CursorStale)
}

pub(crate) async fn collect_complete_group_members_async(
    client: &crate::core::ImClient,
    group: crate::ids::GroupRef,
    expected_version: Option<&str>,
    product_max_members: usize,
) -> crate::ImResult<CompleteGroupMembers> {
    for attempt in 0..MAX_ATTEMPTS {
        match collect_attempt_async(client, group.clone(), expected_version, product_max_members)
            .await
        {
            Err(crate::ImError::CursorStale)
                if expected_version.is_none() && attempt + 1 < MAX_ATTEMPTS =>
            {
                continue
            }
            result => return result,
        }
    }
    Err(crate::ImError::CursorStale)
}

fn collect_attempt(
    client: &crate::core::ImClient,
    group: crate::ids::GroupRef,
    expected_version: Option<&str>,
    product_max_members: usize,
) -> crate::ImResult<CompleteGroupMembers> {
    let mut cursor = None;
    let mut seen_cursors = BTreeSet::new();
    let mut seen_members = BTreeSet::new();
    let mut members = Vec::new();
    let mut version = expected_version.map(str::to_owned);
    let mut frozen_total = None;

    for _ in 0..MAX_PAGES {
        let page = crate::internal::group_runtime::read::GroupReadRuntime::new(
            client,
            crate::internal::auth::session::FileSessionProvider::new(client),
            crate::internal::transport::CoreHttpTransport::new(client),
        )
        .members(crate::groups::GroupMembersRequest {
            group: group.clone(),
            limit: crate::ids::PageLimit(PAGE_LIMIT.into()),
            cursor: cursor.clone(),
        })
        .map_err(map_service_page_error)?;
        let parsed = validate_page(&group, page, version.as_deref(), frozen_total)?;
        version.get_or_insert_with(|| parsed.group_state_version.clone());
        frozen_total.get_or_insert(parsed.total);
        append_unique_members(
            &mut members,
            &mut seen_members,
            parsed.members,
            product_max_members,
        )?;
        if !parsed.has_more {
            return finish(members, version, frozen_total, product_max_members);
        }
        let next = parsed
            .next_cursor
            .ok_or(crate::ImError::InventoryIncomplete)?;
        if !seen_cursors.insert(next.as_str().to_owned()) {
            return Err(crate::ImError::InventoryIncomplete);
        }
        cursor = Some(next);
    }
    Err(crate::ImError::InventoryIncomplete)
}

async fn collect_attempt_async(
    client: &crate::core::ImClient,
    group: crate::ids::GroupRef,
    expected_version: Option<&str>,
    product_max_members: usize,
) -> crate::ImResult<CompleteGroupMembers> {
    let mut cursor = None;
    let mut seen_cursors = BTreeSet::new();
    let mut seen_members = BTreeSet::new();
    let mut members = Vec::new();
    let mut version = expected_version.map(str::to_owned);
    let mut frozen_total = None;

    for _ in 0..MAX_PAGES {
        let page = crate::internal::group_runtime::read::GroupReadRuntime::new(
            client,
            crate::internal::auth::session::FileSessionProvider::new(client),
            crate::internal::transport::CoreHttpTransport::new(client),
        )
        .members_async(crate::groups::GroupMembersRequest {
            group: group.clone(),
            limit: crate::ids::PageLimit(PAGE_LIMIT.into()),
            cursor: cursor.clone(),
        })
        .await
        .map_err(map_service_page_error)?;
        let parsed = validate_page(&group, page, version.as_deref(), frozen_total)?;
        version.get_or_insert_with(|| parsed.group_state_version.clone());
        frozen_total.get_or_insert(parsed.total);
        append_unique_members(
            &mut members,
            &mut seen_members,
            parsed.members,
            product_max_members,
        )?;
        if !parsed.has_more {
            return finish(members, version, frozen_total, product_max_members);
        }
        let next = parsed
            .next_cursor
            .ok_or(crate::ImError::InventoryIncomplete)?;
        if !seen_cursors.insert(next.as_str().to_owned()) {
            return Err(crate::ImError::InventoryIncomplete);
        }
        cursor = Some(next);
    }
    Err(crate::ImError::InventoryIncomplete)
}

struct ValidatedPage {
    members: Vec<crate::groups::GroupMember>,
    group_state_version: String,
    total: u32,
    has_more: bool,
    next_cursor: Option<crate::ids::Cursor>,
}

fn validate_page(
    expected_group: &crate::ids::GroupRef,
    page: crate::groups::GroupReadResult,
    expected_version: Option<&str>,
    expected_total: Option<u32>,
) -> crate::ImResult<ValidatedPage> {
    let raw = page
        .raw_response()
        .ok_or(crate::ImError::InventoryIncomplete)?;
    let raw_members = raw
        .get("members")
        .and_then(Value::as_array)
        .ok_or(crate::ImError::InventoryIncomplete)?;
    if raw_members.len() != page.members.len()
        || raw_members
            .iter()
            .zip(&page.members)
            .any(|(raw_member, typed_member)| {
                let Some(member) = raw_member.as_object() else {
                    return true;
                };
                if member.contains_key("member_user_id")
                    || member.get("status").and_then(Value::as_str) != Some("active")
                {
                    return true;
                }
                let Some(agent_did) = member.get("agent_did").and_then(Value::as_str) else {
                    return true;
                };
                let Ok(agent_did) = crate::ids::Did::parse(agent_did) else {
                    return true;
                };
                agent_did.as_str()
                    != member
                        .get("agent_did")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                    || typed_member.did.as_ref() != Some(&agent_did)
                    || typed_member.status.as_deref() != Some("active")
            })
    {
        return Err(crate::ImError::InventoryIncomplete);
    }
    let page_group = raw
        .get("group_did")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.trim() == *value)
        .ok_or(crate::ImError::InventoryIncomplete)?;
    if page_group != expected_group.as_str() {
        return Err(crate::ImError::InventoryIncomplete);
    }
    let group_state_version = raw
        .get("group_state_version")
        .and_then(Value::as_str)
        .filter(|value| parse_canonical_positive(value).is_some())
        .ok_or(crate::ImError::InventoryIncomplete)?
        .to_owned();
    if expected_version.is_some_and(|expected| expected != group_state_version.as_str()) {
        return Err(crate::ImError::CursorStale);
    }
    let total = raw
        .get("total")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(crate::ImError::InventoryIncomplete)?;
    if expected_total.is_some_and(|expected| expected != total) {
        return Err(crate::ImError::InventoryIncomplete);
    }
    let has_more = raw
        .get("has_more")
        .and_then(Value::as_bool)
        .ok_or(crate::ImError::InventoryIncomplete)?;
    let next_cursor = match (has_more, raw.get("next_cursor")) {
        (true, Some(Value::String(value)))
            if !value.is_empty() && value.trim() == value.as_str() =>
        {
            Some(crate::ids::Cursor::parse(value).map_err(|_| crate::ImError::CursorInvalid)?)
        }
        (false, None) => None,
        _ => return Err(crate::ImError::InventoryIncomplete),
    };
    Ok(ValidatedPage {
        members: page.members,
        group_state_version,
        total,
        has_more,
        next_cursor,
    })
}

fn append_unique_members(
    members: &mut Vec<crate::groups::GroupMember>,
    seen_members: &mut BTreeSet<String>,
    page: Vec<crate::groups::GroupMember>,
    product_max_members: usize,
) -> crate::ImResult<()> {
    for member in page {
        if member.status.as_deref() != Some("active") {
            return Err(crate::ImError::InventoryIncomplete);
        }
        let did = member
            .did
            .as_ref()
            .ok_or(crate::ImError::InventoryIncomplete)?
            .as_str()
            .to_owned();
        if !seen_members.insert(did) {
            return Err(crate::ImError::InventoryIncomplete);
        }
        members.push(member);
        if members.len() > IMPLEMENTATION_HARD_CAP {
            return Err(crate::ImError::InventoryTooLarge);
        }
        if members.len() > product_max_members {
            return Err(crate::ImError::InventoryTooLarge);
        }
    }
    Ok(())
}

fn finish(
    members: Vec<crate::groups::GroupMember>,
    version: Option<String>,
    total: Option<u32>,
    product_max_members: usize,
) -> crate::ImResult<CompleteGroupMembers> {
    let total = total.ok_or(crate::ImError::InventoryIncomplete)?;
    if members.len() != total as usize {
        return Err(crate::ImError::InventoryIncomplete);
    }
    if members.len() > IMPLEMENTATION_HARD_CAP || members.len() > product_max_members {
        return Err(crate::ImError::InventoryTooLarge);
    }
    Ok(CompleteGroupMembers {
        members,
        group_state_version: version.ok_or(crate::ImError::InventoryIncomplete)?,
        total,
    })
}

fn parse_canonical_positive(value: &str) -> Option<u64> {
    if value.is_empty()
        || value.trim() != value
        || value == "0"
        || value.starts_with('0')
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    value.parse().ok()
}

fn map_service_page_error(error: crate::ImError) -> crate::ImError {
    let crate::ImError::Service { code, .. } = &error else {
        return error;
    };
    match code.as_deref() {
        Some("group.local_cursor_invalid") => crate::ImError::CursorInvalid,
        Some("group.local_cursor_stale") => crate::ImError::CursorStale,
        Some("group.local_inventory_incomplete") => crate::ImError::InventoryIncomplete,
        Some("group.local_inventory_too_large") => crate::ImError::InventoryTooLarge,
        _ => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(raw: Value) -> crate::groups::GroupReadResult {
        crate::groups::GroupReadResult::from_raw_response(raw, Vec::new())
    }

    #[test]
    fn strict_member_page_preserves_host_binding_and_cursor() {
        let group = crate::ids::GroupRef::parse("did:example:group").unwrap();
        let parsed = validate_page(
            &group,
            page(serde_json::json!({
                "group_did": group.as_str(),
                "group_state_version": "41",
                "members": [{"agent_did":"did:example:alice","status":"active"}],
                "total": 2,
                "has_more": true,
                "next_cursor": "opaque-page-2"
            })),
            None,
            None,
        )
        .unwrap();
        assert_eq!(parsed.group_state_version, "41");
        assert_eq!(parsed.total, 2);
        assert!(parsed.has_more);
        assert_eq!(
            parsed.next_cursor.as_ref().map(crate::ids::Cursor::as_str),
            Some("opaque-page-2")
        );
    }

    #[test]
    fn strict_member_page_rejects_missing_markers_and_typed_loss() {
        let group = crate::ids::GroupRef::parse("did:example:group").unwrap();
        let missing_version = page(serde_json::json!({
            "group_did": group.as_str(),
            "members": [],
            "total": 0,
            "has_more": false
        }));
        assert!(matches!(
            validate_page(&group, missing_version, None, None),
            Err(crate::ImError::InventoryIncomplete)
        ));

        let invalid_member = page(serde_json::json!({
            "group_did": group.as_str(),
            "group_state_version": "41",
            "members": ["not-an-object"],
            "total": 1,
            "has_more": false
        }));
        assert!(matches!(
            validate_page(&group, invalid_member, None, None),
            Err(crate::ImError::InventoryIncomplete)
        ));
    }

    #[test]
    fn strict_member_page_detects_revision_change() {
        let group = crate::ids::GroupRef::parse("did:example:group").unwrap();
        let changed = page(serde_json::json!({
            "group_did": group.as_str(),
            "group_state_version": "42",
            "members": [],
            "total": 0,
            "has_more": false
        }));
        assert!(matches!(
            validate_page(&group, changed, Some("41"), Some(0)),
            Err(crate::ImError::CursorStale)
        ));
    }

    #[test]
    fn strict_member_page_rejects_noncanonical_security_fields() {
        let group = crate::ids::GroupRef::parse("did:example:group").unwrap();
        for raw in [
            serde_json::json!({
                "group_did": group.as_str(),
                "group_state_version": " 41 ",
                "members": [],
                "total": 0,
                "has_more": false
            }),
            serde_json::json!({
                "group_did": group.as_str(),
                "group_state_version": "41",
                "members": [{"agent_did":"did:example:alice","status":"active"}],
                "total": 1,
                "has_more": true,
                "next_cursor": " opaque-page-2 "
            }),
            serde_json::json!({
                "group_did": group.as_str(),
                "group_state_version": "41",
                "members": [{"agent_did":"did:example:alice"}],
                "total": 1,
                "has_more": false
            }),
            serde_json::json!({
                "group_did": group.as_str(),
                "group_state_version": "41",
                "members": [{"agent_did":"not-a-did","status":"active"}],
                "total": 1,
                "has_more": false
            }),
        ] {
            assert!(matches!(
                validate_page(&group, page(raw), None, None),
                Err(crate::ImError::InventoryIncomplete)
            ));
        }
    }

    #[test]
    fn canonical_group_members_fixture_is_consumed_by_the_strict_collector() {
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/multi_device_v1/group-members-page-1.json");
        let fixture: Value = serde_json::from_slice(&std::fs::read(fixture_path).unwrap()).unwrap();
        let raw = fixture.pointer("/response/result").cloned().unwrap();
        let group =
            crate::ids::GroupRef::parse(raw.get("group_did").and_then(Value::as_str).unwrap())
                .unwrap();

        let parsed = validate_page(&group, page(raw), None, None).unwrap();

        assert_eq!(parsed.group_state_version, "41");
        assert_eq!(parsed.total, 3);
        assert!(parsed.has_more);
        assert_eq!(parsed.members.len(), 2);
        assert!(parsed
            .members
            .iter()
            .all(|member| member.did.is_some() && member.status.as_deref() == Some("active")));
    }
}
