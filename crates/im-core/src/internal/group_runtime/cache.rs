use serde_json::Value;

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
pub(crate) fn cached_group_snapshot(
    client: &crate::core::ImClient,
    group_did: &str,
) -> crate::ImResult<Option<Value>> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    for owner_identity_id in owner_identity_ids(client) {
        if let Some(snapshot) =
            crate::internal::local_state::groups::get_group_snapshot_for_owner_identity(
                &connection,
                &owner_identity_id,
                client.did().as_str(),
                &group_storage_key(group_did),
            )?
        {
            return Ok(Some(enrich_cached_group_snapshot(snapshot)));
        }
    }
    Ok(None)
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
pub(crate) fn cached_group_snapshot(
    _client: &crate::core::ImClient,
    _group_did: &str,
) -> crate::ImResult<Option<Value>> {
    Ok(None)
}

#[cfg(not(feature = "sqlite"))]
pub(crate) fn cached_group_snapshot(
    _client: &crate::core::ImClient,
    _group_did: &str,
) -> crate::ImResult<Option<Value>> {
    Ok(None)
}

#[cfg(feature = "sqlite")]
pub(crate) async fn cached_group_snapshot_async(
    client: &crate::core::ImClient,
    group_did: &str,
) -> crate::ImResult<Option<Value>> {
    let db = client.core_inner().local_state_db().await?;
    for owner_identity_id in owner_identity_ids(client) {
        if let Some(snapshot) = db
            .get_group_snapshot(
                owner_identity_id,
                client.did().as_str(),
                group_storage_key(group_did),
            )
            .await?
        {
            return Ok(Some(enrich_cached_group_snapshot(snapshot)));
        }
    }
    Ok(None)
}

#[cfg(not(feature = "sqlite"))]
pub(crate) async fn cached_group_snapshot_async(
    _client: &crate::core::ImClient,
    _group_did: &str,
) -> crate::ImResult<Option<Value>> {
    Ok(None)
}

pub(crate) fn is_active_group_owner(snapshot: &Value) -> bool {
    let role = default_string(
        &string_value(snapshot.get("my_role")),
        &string_value(snapshot.get("member_role")),
    );
    let status = default_string(
        &string_value(snapshot.get("membership_status")),
        &string_value(snapshot.get("member_status")),
    );
    role == "owner" && status == "active"
}

pub(crate) fn group_snapshot_uses_e2ee(snapshot: &Value) -> bool {
    if snapshot.is_null() {
        return false;
    }
    if value_string(snapshot.get("message_security_profile")) == "group-e2ee"
        || value_string(snapshot.get("required_security_profile")) == "group-e2ee"
    {
        return true;
    }
    if snapshot
        .get("group_policy")
        .and_then(Value::as_object)
        .map(|policy| value_string(policy.get("message_security_profile")))
        .is_some_and(|profile| profile == "group-e2ee")
    {
        return true;
    }
    decoded_metadata(snapshot).as_ref().is_some_and(|metadata| {
        value_string(metadata.get("message_security_profile")) == "group-e2ee"
            || value_string(metadata.get("required_security_profile")) == "group-e2ee"
    })
}

#[cfg(feature = "sqlite")]
fn owner_identity_ids(client: &crate::core::ImClient) -> Vec<String> {
    let mut ids = Vec::new();
    push_nonempty_unique(&mut ids, client.current_identity().id.as_str());
    if let Some(alias) = client.current_identity().local_alias.as_deref() {
        push_nonempty_unique(&mut ids, alias);
    }
    ids
}

#[cfg(feature = "sqlite")]
fn push_nonempty_unique(values: &mut Vec<String>, value: &str) {
    let value = value.trim();
    if value.is_empty() || values.iter().any(|known| known == value) {
        return;
    }
    values.push(value.to_string());
}

#[cfg(feature = "sqlite")]
fn enrich_cached_group_snapshot(mut snapshot: Value) -> Value {
    let metadata = snapshot
        .get("metadata")
        .and_then(Value::as_str)
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
        .and_then(|value| normalize_group_snapshot(&value));
    if let (Some(object), Some(Value::Object(metadata_object))) =
        (snapshot.as_object_mut(), metadata)
    {
        for (key, value) in metadata_object {
            object.entry(key).or_insert(value);
        }
    }
    if let Some(object) = snapshot.as_object_mut() {
        let group_did = string_value(object.get("group_did"));
        if !group_did.trim().is_empty() {
            object
                .entry("did".to_string())
                .or_insert(Value::String(group_did));
        }
        let my_role = string_value(object.get("my_role"));
        if !my_role.trim().is_empty() {
            object
                .entry("member_role".to_string())
                .or_insert(Value::String(my_role));
        }
        let status = string_value(object.get("membership_status"));
        if !status.trim().is_empty() {
            object
                .entry("member_status".to_string())
                .or_insert(Value::String(status));
        }
        if !object.contains_key("group_profile") {
            let mut profile = serde_json::Map::new();
            insert_from_object(object, &mut profile, "display_name", "name");
            insert_from_object(object, &mut profile, "description", "description");
            insert_from_object(object, &mut profile, "slug", "slug");
            insert_from_object(object, &mut profile, "goal", "goal");
            insert_from_object(object, &mut profile, "rules", "rules");
            insert_from_object(object, &mut profile, "message_prompt", "message_prompt");
            insert_from_object(object, &mut profile, "doc_url", "doc_url");
            if !profile.is_empty() {
                object.insert("group_profile".to_string(), Value::Object(profile));
            }
        }
    }
    snapshot
}

#[cfg(feature = "sqlite")]
fn insert_from_object(
    source: &serde_json::Map<String, Value>,
    target: &mut serde_json::Map<String, Value>,
    target_key: &str,
    source_key: &str,
) {
    if let Some(value) = source.get(source_key).filter(|value| !value.is_null()) {
        target.insert(target_key.to_string(), value.clone());
    }
}

fn normalize_group_snapshot(raw: &Value) -> Option<Value> {
    if raw.is_null() {
        return None;
    }
    if let Some(snapshot) = raw.get("group_snapshot").filter(|value| value.is_object()) {
        return Some(snapshot.clone());
    }
    let group_did = group_did_from_result(raw);
    if group_did.trim().is_empty() {
        return None;
    }
    if let Some(profile) = raw.get("group_profile").filter(|value| value.is_object()) {
        return Some(serde_json::json!({
            "group_did": group_did,
            "did": group_did,
            "group_state_version": raw.get("group_state_version").cloned().unwrap_or(Value::Null),
            "name": string_value(profile.get("display_name")),
            "description": profile.get("description").cloned().unwrap_or(Value::Null),
            "discoverability": profile.get("discoverability").cloned().unwrap_or(Value::Null),
            "slug": profile.get("slug").cloned().unwrap_or(Value::Null),
            "goal": profile.get("goal").cloned().unwrap_or(Value::Null),
            "rules": profile.get("rules").cloned().unwrap_or(Value::Null),
            "message_prompt": profile.get("message_prompt").cloned().unwrap_or(Value::Null),
            "doc_url": profile.get("doc_url").cloned().unwrap_or(Value::Null),
            "owner_did": raw.get("owner_did").cloned().unwrap_or(Value::Null),
            "member_role": raw.get("member_role").or_else(|| raw.get("my_role")).cloned().unwrap_or(Value::Null),
            "my_role": raw.get("my_role").or_else(|| raw.get("member_role")).cloned().unwrap_or(Value::Null),
            "member_status": raw.get("member_status").or_else(|| raw.get("membership_status")).cloned().unwrap_or(Value::Null),
            "membership_status": raw.get("membership_status").or_else(|| raw.get("member_status")).cloned().unwrap_or(Value::Null),
            "join_enabled": raw.get("join_enabled").cloned().unwrap_or(Value::Null),
            "member_count": raw.get("member_count").cloned().unwrap_or(Value::Null),
            "group_profile": profile,
            "group_policy": raw.get("group_policy").cloned().unwrap_or(Value::Null),
            "created_at": raw.get("created_at").cloned().unwrap_or(Value::Null),
            "updated_at": raw.get("updated_at").cloned().unwrap_or(Value::Null),
        }));
    }
    Some(raw.clone())
}

fn decoded_metadata(snapshot: &Value) -> Option<Value> {
    snapshot
        .get("metadata")
        .and_then(Value::as_str)
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
}

fn group_did_from_result(raw: &Value) -> String {
    string_value(raw.get("group_did"))
        .trim()
        .to_string()
        .or_else_nonempty(|| string_value(raw.get("did")))
}

#[cfg(feature = "sqlite")]
fn group_storage_key(group_did: &str) -> String {
    group_did.trim().to_string()
}

fn string_value(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn default_string(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn value_string(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

trait NonEmptyString {
    fn or_else_nonempty(self, fallback: impl FnOnce() -> String) -> String;
}

impl NonEmptyString for String {
    fn or_else_nonempty(self, fallback: impl FnOnce() -> String) -> String {
        if self.trim().is_empty() {
            fallback()
        } else {
            self
        }
    }
}
