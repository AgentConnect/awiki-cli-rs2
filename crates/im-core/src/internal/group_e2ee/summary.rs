#[cfg(feature = "sqlite")]
use serde_json::{Map, Value};

#[derive(Debug, Clone)]
pub(crate) struct GroupE2eeSummaryUpdate<'a> {
    pub(crate) group_did: &'a str,
    pub(crate) epoch: Option<&'a str>,
    pub(crate) group_state_version: Option<&'a str>,
    pub(crate) crypto_group_id_b64u: Option<&'a str>,
    pub(crate) epoch_authenticator: Option<&'a str>,
    pub(crate) suite: Option<&'a str>,
    pub(crate) operation_id: Option<&'a str>,
    pub(crate) membership_status: &'a str,
}

#[cfg(all(feature = "sqlite", feature = "blocking"))]
pub(crate) fn persist_group_e2ee_summary(
    client: &crate::core::ImClient,
    update: GroupE2eeSummaryUpdate<'_>,
) {
    let Ok(connection) = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    ) else {
        return;
    };
    let _ = crate::internal::local_state::groups::upsert_group_e2ee_summary(
        &connection,
        group_e2ee_summary_record(client, &update),
    );
}

#[cfg(all(feature = "sqlite", not(feature = "blocking")))]
pub(crate) fn persist_group_e2ee_summary(
    _client: &crate::core::ImClient,
    _update: GroupE2eeSummaryUpdate<'_>,
) {
}

#[cfg(not(feature = "sqlite"))]
pub(crate) fn persist_group_e2ee_summary(
    _client: &crate::core::ImClient,
    _update: GroupE2eeSummaryUpdate<'_>,
) {
}

#[cfg(feature = "sqlite")]
pub(crate) async fn persist_group_e2ee_summary_async(
    client: &crate::core::ImClient,
    update: GroupE2eeSummaryUpdate<'_>,
) -> crate::ImResult<()> {
    let record = group_e2ee_summary_record(client, &update);
    client
        .core_inner()
        .local_state_db()
        .await?
        .upsert_group_e2ee_summary(record)
        .await
}

#[cfg(not(feature = "sqlite"))]
pub(crate) async fn persist_group_e2ee_summary_async(
    _client: &crate::core::ImClient,
    _update: GroupE2eeSummaryUpdate<'_>,
) -> crate::ImResult<()> {
    Ok(())
}

#[cfg(feature = "sqlite")]
fn group_e2ee_summary_record(
    client: &crate::core::ImClient,
    update: &GroupE2eeSummaryUpdate<'_>,
) -> crate::internal::local_state::groups::GroupE2eeSummaryRecord {
    let mut group_e2ee = Map::new();
    insert_string(
        &mut group_e2ee,
        "crypto_group_id_b64u",
        update.crypto_group_id_b64u,
    );
    insert_string(&mut group_e2ee, "epoch", update.epoch);
    insert_string(
        &mut group_e2ee,
        "epoch_authenticator",
        update.epoch_authenticator,
    );
    insert_string(&mut group_e2ee, "suite", update.suite);
    insert_string(
        &mut group_e2ee,
        "group_state_version",
        update.group_state_version,
    );
    insert_string(&mut group_e2ee, "operation_id", update.operation_id);
    let updated_at = crate::internal::wire::common::now_rfc3339();
    insert_string(&mut group_e2ee, "updated_at", Some(updated_at.as_str()));

    let mut metadata = Map::new();
    metadata.insert(
        "message_security_profile".to_owned(),
        Value::String(super::wire::GROUP_E2EE_SECURITY_PROFILE.to_owned()),
    );
    metadata.insert("group_e2ee".to_owned(), Value::Object(group_e2ee));
    if let Some(group_state_version) = update
        .group_state_version
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        metadata.insert(
            "group_state_version".to_owned(),
            Value::String(group_state_version.to_owned()),
        );
    }

    let record = crate::internal::local_state::groups::GroupRecord {
        owner_identity_id: client.current_identity().id.as_str().to_owned(),
        owner_did: client.did().as_str().to_owned(),
        group_id: update.group_did.to_owned(),
        group_did: update.group_did.to_owned(),
        membership_status: if update.membership_status.trim().is_empty() {
            "active".to_owned()
        } else {
            update.membership_status.trim().to_owned()
        },
        metadata: Value::Object(metadata).to_string(),
        credential_name: client.current_identity().id.as_str().to_owned(),
        ..crate::internal::local_state::groups::GroupRecord::default()
    };
    crate::internal::local_state::groups::GroupE2eeSummaryRecord {
        record,
        epoch: update
            .epoch
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        group_state_version: update
            .group_state_version
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
    }
}

#[cfg(feature = "sqlite")]
fn insert_string(map: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        map.insert(key.to_owned(), Value::String(value.to_owned()));
    }
}
