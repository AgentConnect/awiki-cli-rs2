use serde_json::{Map, Value};

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
pub(crate) fn persist_messages(
    client: &crate::core::ImClient,
    messages: &[crate::messages::Message],
) -> crate::ImResult<()> {
    if messages.is_empty() {
        return Ok(());
    }
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    let records = messages
        .iter()
        .map(|message| message_record_from_message(client, message))
        .collect::<crate::ImResult<Vec<_>>>()?;
    crate::internal::local_state::messages::upsert_messages(&connection, &records)
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
pub(crate) fn persist_messages(
    _client: &crate::core::ImClient,
    _messages: &[crate::messages::Message],
) -> crate::ImResult<()> {
    Err(crate::ImError::unsupported("sync-message-projection"))
}

#[cfg(feature = "sqlite")]
pub(crate) async fn persist_messages_async(
    client: &crate::core::ImClient,
    messages: &[crate::messages::Message],
) -> crate::ImResult<()> {
    if messages.is_empty() {
        return Ok(());
    }
    let records = messages
        .iter()
        .map(|message| message_record_from_message(client, message))
        .collect::<crate::ImResult<Vec<_>>>()?;
    client
        .core_inner()
        .local_state_db()
        .await?
        .store_messages(records)
        .await
}

#[cfg(not(feature = "sqlite"))]
pub(crate) fn persist_messages(
    _client: &crate::core::ImClient,
    _messages: &[crate::messages::Message],
) -> crate::ImResult<()> {
    Ok(())
}

#[cfg(not(feature = "sqlite"))]
pub(crate) async fn persist_messages_async(
    _client: &crate::core::ImClient,
    _messages: &[crate::messages::Message],
) -> crate::ImResult<()> {
    Ok(())
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
pub(crate) fn persist_direct_outgoing_result(
    client: &crate::core::ImClient,
    target_did: &str,
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::messages::upsert_message(
        &connection,
        &direct_outgoing_result_record(client, target_did, target_handle, peer_scope, sdk_result),
    )
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
pub(crate) fn persist_direct_outgoing_result(
    _client: &crate::core::ImClient,
    _target_did: &str,
    _target_handle: Option<&str>,
    _peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    _sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    Err(crate::ImError::unsupported("sync-message-projection"))
}

#[cfg(feature = "sqlite")]
pub(crate) async fn persist_direct_outgoing_result_async(
    client: &crate::core::ImClient,
    target_did: &str,
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    let record =
        direct_outgoing_result_record(client, target_did, target_handle, peer_scope, sdk_result);
    client
        .core_inner()
        .local_state_db()
        .await?
        .store_messages(vec![record])
        .await
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
pub(crate) fn persist_direct_outgoing(
    client: &crate::core::ImClient,
    target_did: &str,
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    text: &str,
    kind: &crate::messages::MessageKind,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::messages::upsert_message(
        &connection,
        &direct_outgoing_record(
            client,
            target_did,
            target_handle,
            peer_scope,
            text,
            kind,
            sdk_result,
        ),
    )
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
pub(crate) fn persist_direct_outgoing(
    _client: &crate::core::ImClient,
    _target_did: &str,
    _target_handle: Option<&str>,
    _peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    _text: &str,
    _kind: &crate::messages::MessageKind,
    _sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    Err(crate::ImError::unsupported("sync-message-projection"))
}

#[cfg(feature = "sqlite")]
pub(crate) async fn persist_direct_outgoing_async(
    client: &crate::core::ImClient,
    target_did: &str,
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    text: &str,
    kind: &crate::messages::MessageKind,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    let record = direct_outgoing_record(
        client,
        target_did,
        target_handle,
        peer_scope,
        text,
        kind,
        sdk_result,
    );
    client
        .core_inner()
        .local_state_db()
        .await?
        .store_messages(vec![record])
        .await
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
pub(crate) fn persist_group_outgoing_result(
    client: &crate::core::ImClient,
    group_did: &str,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::messages::upsert_message(
        &connection,
        &group_outgoing_result_record(client, group_did, sdk_result),
    )?;
    touch_group_after_outgoing(&connection, client, group_did, sdk_result)
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
pub(crate) fn persist_group_outgoing_result(
    _client: &crate::core::ImClient,
    _group_did: &str,
    _sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    Err(crate::ImError::unsupported("sync-message-projection"))
}

#[cfg(feature = "sqlite")]
pub(crate) async fn persist_group_outgoing_result_async(
    client: &crate::core::ImClient,
    group_did: &str,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    let db = client.core_inner().local_state_db().await?;
    db.store_messages(vec![group_outgoing_result_record(
        client, group_did, sdk_result,
    )])
    .await?;
    db.upsert_group(group_touch_record(client, group_did, sdk_result))
        .await
}

#[cfg(feature = "sqlite")]
pub(crate) async fn persist_send_projection_async(
    client: &crate::core::ImClient,
    target: &crate::messages::MessageTarget,
    body: &crate::messages::MessageBody,
    message_id: &crate::ids::MessageId,
    operation_id: Option<&str>,
    delivery: crate::messages::DeliveryState,
    target_did: Option<&str>,
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
) -> crate::ImResult<crate::messages::SendMessageResult> {
    let sdk_result = send_projection_result(
        client,
        target,
        body,
        message_id,
        operation_id,
        delivery,
        target_did,
        target_handle,
        peer_scope,
    )?;
    let record = send_projection_record(
        client,
        target,
        body,
        &sdk_result,
        target_did,
        target_handle,
        peer_scope,
    )?;
    client
        .core_inner()
        .local_state_db()
        .await?
        .store_messages(vec![record])
        .await?;
    Ok(sdk_result)
}

#[cfg(not(feature = "sqlite"))]
pub(crate) async fn persist_send_projection_async(
    client: &crate::core::ImClient,
    target: &crate::messages::MessageTarget,
    body: &crate::messages::MessageBody,
    message_id: &crate::ids::MessageId,
    operation_id: Option<&str>,
    delivery: crate::messages::DeliveryState,
    target_did: Option<&str>,
    _target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
) -> crate::ImResult<crate::messages::SendMessageResult> {
    send_projection_result(
        client,
        target,
        body,
        message_id,
        operation_id,
        delivery,
        target_did,
        _target_handle,
        peer_scope,
    )
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
pub(crate) fn persist_group_outgoing(
    client: &crate::core::ImClient,
    group_did: &str,
    text: &str,
    kind: &crate::messages::MessageKind,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::messages::upsert_message(
        &connection,
        &group_outgoing_record(client, group_did, text, kind, sdk_result),
    )?;
    touch_group_after_outgoing(&connection, client, group_did, sdk_result)
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
pub(crate) fn persist_group_outgoing(
    _client: &crate::core::ImClient,
    _group_did: &str,
    _text: &str,
    _kind: &crate::messages::MessageKind,
    _sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    Err(crate::ImError::unsupported("sync-message-projection"))
}

#[cfg(feature = "sqlite")]
pub(crate) async fn persist_group_outgoing_async(
    client: &crate::core::ImClient,
    group_did: &str,
    text: &str,
    kind: &crate::messages::MessageKind,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    let db = client.core_inner().local_state_db().await?;
    db.store_messages(vec![group_outgoing_record(
        client, group_did, text, kind, sdk_result,
    )])
    .await?;
    db.upsert_group(group_touch_record(client, group_did, sdk_result))
        .await
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
pub(crate) fn persist_direct_attachment_outgoing(
    client: &crate::core::ImClient,
    target_did: &str,
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    manifest: &Value,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::messages::upsert_message(
        &connection,
        &direct_attachment_outgoing_record(
            client,
            target_did,
            target_handle,
            peer_scope,
            manifest,
            sdk_result,
        ),
    )
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
pub(crate) fn persist_direct_attachment_outgoing(
    _client: &crate::core::ImClient,
    _target_did: &str,
    _target_handle: Option<&str>,
    _peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    _manifest: &Value,
    _sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    Err(crate::ImError::unsupported("sync-message-projection"))
}

#[cfg(feature = "sqlite")]
pub(crate) async fn persist_direct_attachment_outgoing_async(
    client: &crate::core::ImClient,
    target_did: &str,
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    manifest: &Value,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    let record = direct_attachment_outgoing_record(
        client,
        target_did,
        target_handle,
        peer_scope,
        manifest,
        sdk_result,
    );
    client
        .core_inner()
        .local_state_db()
        .await?
        .store_messages(vec![record])
        .await
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
pub(crate) fn persist_group_attachment_outgoing(
    client: &crate::core::ImClient,
    group_did: &str,
    manifest: &Value,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::messages::upsert_message(
        &connection,
        &group_attachment_outgoing_record(client, group_did, manifest, sdk_result),
    )?;
    touch_group_after_outgoing(&connection, client, group_did, sdk_result)
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
pub(crate) fn persist_group_attachment_outgoing(
    _client: &crate::core::ImClient,
    _group_did: &str,
    _manifest: &Value,
    _sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    Err(crate::ImError::unsupported("sync-message-projection"))
}

#[cfg(feature = "sqlite")]
pub(crate) async fn persist_group_attachment_outgoing_async(
    client: &crate::core::ImClient,
    group_did: &str,
    manifest: &Value,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    let db = client.core_inner().local_state_db().await?;
    db.store_messages(vec![group_attachment_outgoing_record(
        client, group_did, manifest, sdk_result,
    )])
    .await?;
    db.upsert_group(group_touch_record(client, group_did, sdk_result))
        .await
}

#[cfg(not(feature = "sqlite"))]
pub(crate) async fn persist_direct_attachment_outgoing_async(
    _client: &crate::core::ImClient,
    _target_did: &str,
    _target_handle: Option<&str>,
    _peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    _manifest: &Value,
    _sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    Ok(())
}

#[cfg(not(feature = "sqlite"))]
pub(crate) async fn persist_group_attachment_outgoing_async(
    _client: &crate::core::ImClient,
    _group_did: &str,
    _manifest: &Value,
    _sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    Ok(())
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
pub(crate) fn peer_dids_for_handle(
    client: &crate::core::ImClient,
    handle: &str,
    current_did: &str,
) -> crate::ImResult<Vec<String>> {
    let connection = crate::internal::contact_store::open_writable(client)?;
    let dids = crate::internal::contact_store::records::list_dids_by_handle_for_owner_identity(
        &connection,
        client.current_identity().id.as_str(),
        client.did().as_str(),
        &normalize_handle_value(handle),
    )?;
    Ok(merge_peer_dids(current_did, &dids))
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
pub(crate) fn peer_dids_for_handle(
    _client: &crate::core::ImClient,
    _handle: &str,
    _current_did: &str,
) -> crate::ImResult<Vec<String>> {
    Err(crate::ImError::unsupported("sync-message-projection"))
}

#[cfg(feature = "sqlite")]
pub(crate) async fn peer_dids_for_handle_async(
    client: &crate::core::ImClient,
    handle: &str,
    current_did: &str,
) -> crate::ImResult<Vec<String>> {
    let dids = client
        .core_inner()
        .local_state_db()
        .await?
        .list_dids_by_handle(
            client.current_identity().id.as_str(),
            client.did().as_str(),
            normalize_handle_value(handle),
        )
        .await?;
    Ok(merge_peer_dids(current_did, &dids))
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
pub(crate) fn persist_direct_e2ee_outgoing(
    connection: &rusqlite::Connection,
    client: &crate::core::ImClient,
    target_did: &str,
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    text: &str,
    kind: &crate::messages::MessageKind,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    crate::internal::local_state::messages::upsert_message(
        connection,
        &direct_e2ee_outgoing_record(
            client,
            target_did,
            target_handle,
            peer_scope,
            text,
            kind,
            sdk_result,
        ),
    )
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
pub(crate) fn persist_direct_e2ee_outgoing(
    _connection: &rusqlite::Connection,
    _client: &crate::core::ImClient,
    _target_did: &str,
    _target_handle: Option<&str>,
    _peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    _text: &str,
    _kind: &crate::messages::MessageKind,
    _sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    Err(crate::ImError::unsupported("sync-message-projection"))
}

#[cfg(feature = "sqlite")]
pub(crate) async fn persist_direct_e2ee_outgoing_async(
    client: &crate::core::ImClient,
    target_did: &str,
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    text: &str,
    kind: &crate::messages::MessageKind,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    let record = direct_e2ee_outgoing_record(
        client,
        target_did,
        target_handle,
        peer_scope,
        text,
        kind,
        sdk_result,
    );
    client
        .core_inner()
        .local_state_db()
        .await?
        .store_messages(vec![record])
        .await
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
pub(crate) fn persist_direct_e2ee_attachment_outgoing(
    connection: &rusqlite::Connection,
    client: &crate::core::ImClient,
    target_did: &str,
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    redacted_manifest: &Value,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    crate::internal::local_state::messages::upsert_message(
        connection,
        &direct_e2ee_attachment_outgoing_record(
            client,
            target_did,
            target_handle,
            peer_scope,
            redacted_manifest,
            sdk_result,
        ),
    )
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
pub(crate) fn persist_direct_e2ee_attachment_outgoing(
    _connection: &rusqlite::Connection,
    _client: &crate::core::ImClient,
    _target_did: &str,
    _target_handle: Option<&str>,
    _peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    _redacted_manifest: &Value,
    _sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    Err(crate::ImError::unsupported(
        "sync-direct-e2ee-attachment-projection",
    ))
}

#[cfg(feature = "sqlite")]
pub(crate) async fn persist_direct_e2ee_attachment_outgoing_async(
    client: &crate::core::ImClient,
    target_did: &str,
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    redacted_manifest: &Value,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    let record = direct_e2ee_attachment_outgoing_record(
        client,
        target_did,
        target_handle,
        peer_scope,
        redacted_manifest,
        sdk_result,
    );
    client
        .core_inner()
        .local_state_db()
        .await?
        .store_messages(vec![record])
        .await
}

#[cfg(all(feature = "group-e2ee", any(feature = "blocking", test)))]
pub(crate) fn persist_group_e2ee_outgoing(
    client: &crate::core::ImClient,
    group_did: &str,
    text: &str,
    kind: &crate::messages::MessageKind,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::messages::upsert_message(
        &connection,
        &group_e2ee_outgoing_record(client, group_did, text, kind, sdk_result),
    )
}

#[cfg(all(feature = "group-e2ee", not(any(feature = "blocking", test))))]
pub(crate) fn persist_group_e2ee_outgoing(
    _client: &crate::core::ImClient,
    _group_did: &str,
    _text: &str,
    _kind: &crate::messages::MessageKind,
    _sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    Err(crate::ImError::unsupported("sync-message-projection"))
}

#[cfg(feature = "group-e2ee")]
pub(crate) async fn persist_group_e2ee_outgoing_async(
    client: &crate::core::ImClient,
    group_did: &str,
    text: &str,
    kind: &crate::messages::MessageKind,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    let record = group_e2ee_outgoing_record(client, group_did, text, kind, sdk_result);
    client
        .core_inner()
        .local_state_db()
        .await?
        .store_messages(vec![record])
        .await
}

#[cfg(all(feature = "group-e2ee", any(feature = "blocking", test)))]
pub(crate) fn persist_group_e2ee_payload_outgoing(
    client: &crate::core::ImClient,
    group_did: &str,
    payload: &Value,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::messages::upsert_message(
        &connection,
        &group_e2ee_payload_outgoing_record(client, group_did, payload, sdk_result),
    )
}

#[cfg(all(feature = "group-e2ee", not(any(feature = "blocking", test))))]
pub(crate) fn persist_group_e2ee_payload_outgoing(
    _client: &crate::core::ImClient,
    _group_did: &str,
    _payload: &Value,
    _sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    Err(crate::ImError::unsupported(
        "sync-group-e2ee-payload-projection",
    ))
}

#[cfg(feature = "group-e2ee")]
pub(crate) async fn persist_group_e2ee_payload_outgoing_async(
    client: &crate::core::ImClient,
    group_did: &str,
    payload: &Value,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    let record = group_e2ee_payload_outgoing_record(client, group_did, payload, sdk_result);
    client
        .core_inner()
        .local_state_db()
        .await?
        .store_messages(vec![record])
        .await
}

#[cfg(all(feature = "group-e2ee", any(feature = "blocking", test)))]
pub(crate) fn persist_group_e2ee_attachment_outgoing(
    client: &crate::core::ImClient,
    group_did: &str,
    redacted_manifest: &Value,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::messages::upsert_message(
        &connection,
        &group_e2ee_attachment_outgoing_record(client, group_did, redacted_manifest, sdk_result),
    )
}

#[cfg(all(feature = "group-e2ee", not(any(feature = "blocking", test))))]
pub(crate) fn persist_group_e2ee_attachment_outgoing(
    _client: &crate::core::ImClient,
    _group_did: &str,
    _redacted_manifest: &Value,
    _sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    Err(crate::ImError::unsupported(
        "sync-group-e2ee-attachment-projection",
    ))
}

#[cfg(feature = "group-e2ee")]
pub(crate) async fn persist_group_e2ee_attachment_outgoing_async(
    client: &crate::core::ImClient,
    group_did: &str,
    redacted_manifest: &Value,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    let record =
        group_e2ee_attachment_outgoing_record(client, group_did, redacted_manifest, sdk_result);
    client
        .core_inner()
        .local_state_db()
        .await?
        .store_messages(vec![record])
        .await
}

#[cfg(feature = "group-e2ee")]
fn group_e2ee_outgoing_record(
    client: &crate::core::ImClient,
    group_did: &str,
    text: &str,
    kind: &crate::messages::MessageKind,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::internal::local_state::messages::MessageRecord {
    let conversation_id = group_conversation_id(group_did);
    crate::internal::local_state::messages::MessageRecord {
        msg_id: sdk_result.message.id.as_str().to_owned(),
        owner_identity_id: client.current_identity().id.as_str().to_owned(),
        owner_did: client.did().as_str().to_owned(),
        conversation_id: conversation_id.clone(),
        thread_id: conversation_id,
        direction: 1,
        sender_did: client.did().as_str().to_owned(),
        group_id: group_did.trim().to_owned(),
        group_did: group_did.trim().to_owned(),
        content_type: content_type_for_kind(kind).to_owned(),
        content: text.to_owned(),
        server_seq: sdk_result.message.metadata.server_sequence,
        sent_at: sdk_result.message.sent_at.clone().unwrap_or_default(),
        is_e2ee: true,
        is_read: true,
        metadata: secure_metadata_json_without_extras("group-e2ee", &sdk_result.message.metadata),
        credential_name: credential_name(client),
        ..crate::internal::local_state::messages::MessageRecord::default()
    }
}

#[cfg(feature = "group-e2ee")]
fn group_e2ee_payload_outgoing_record(
    client: &crate::core::ImClient,
    group_did: &str,
    payload: &Value,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::internal::local_state::messages::MessageRecord {
    let conversation_id = group_conversation_id(group_did);
    crate::internal::local_state::messages::MessageRecord {
        msg_id: sdk_result.message.id.as_str().to_owned(),
        owner_identity_id: client.current_identity().id.as_str().to_owned(),
        owner_did: client.did().as_str().to_owned(),
        conversation_id: conversation_id.clone(),
        thread_id: conversation_id,
        direction: 1,
        sender_did: client.did().as_str().to_owned(),
        group_id: group_storage_key(group_did),
        group_did: group_did.trim().to_owned(),
        content_type: "application/json".to_owned(),
        content: serde_json::to_string(payload).unwrap_or_default(),
        server_seq: sdk_result.message.metadata.server_sequence,
        sent_at: sdk_result.message.sent_at.clone().unwrap_or_default(),
        is_e2ee: true,
        is_read: true,
        metadata: secure_metadata_json_without_extras("group-e2ee", &sdk_result.message.metadata),
        credential_name: credential_name(client),
        ..crate::internal::local_state::messages::MessageRecord::default()
    }
}

#[cfg(feature = "group-e2ee")]
fn group_e2ee_attachment_outgoing_record(
    client: &crate::core::ImClient,
    group_did: &str,
    redacted_manifest: &Value,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::internal::local_state::messages::MessageRecord {
    let conversation_id = group_conversation_id(group_did);
    crate::internal::local_state::messages::MessageRecord {
        msg_id: sdk_result.message.id.as_str().to_owned(),
        owner_identity_id: client.current_identity().id.as_str().to_owned(),
        owner_did: client.did().as_str().to_owned(),
        conversation_id: conversation_id.clone(),
        thread_id: conversation_id,
        direction: 1,
        sender_did: client.did().as_str().to_owned(),
        group_id: group_storage_key(group_did),
        group_did: group_did.trim().to_owned(),
        content_type: crate::attachments::manifest::attachment_manifest_content_type().to_owned(),
        content: crate::attachments::manifest::manifest_content_string(redacted_manifest),
        server_seq: sdk_result.message.metadata.server_sequence,
        sent_at: sdk_result.message.sent_at.clone().unwrap_or_default(),
        is_e2ee: true,
        is_read: true,
        metadata: secure_metadata_json_without_extras("group-e2ee", &sdk_result.message.metadata),
        credential_name: credential_name(client),
        ..crate::internal::local_state::messages::MessageRecord::default()
    }
}

pub(crate) fn direct_conversation_id(peer_did: &str) -> String {
    crate::internal::local_state::owner_scope::direct_conversation_id(peer_did)
}

pub(crate) fn direct_conversation_id_for_peer_scope(
    scope: &crate::internal::local_state::owner_scope::DirectPeerScope,
) -> String {
    crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(scope)
}

pub(crate) fn scoped_direct_thread_ref_from_metadata(
    metadata: &crate::messages::MessageMetadata,
) -> Option<crate::messages::ThreadRef> {
    peer_scope_from_metadata(metadata).and_then(|scope| {
        crate::ids::ThreadId::parse(direct_conversation_id_for_peer_scope(&scope))
            .ok()
            .map(crate::messages::ThreadRef::Thread)
    })
}

#[cfg(feature = "sqlite")]
fn direct_conversation_id_for_scope_or_did(
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    peer_did: &str,
) -> String {
    peer_scope
        .map(direct_conversation_id_for_peer_scope)
        .unwrap_or_else(|| direct_conversation_id(peer_did))
}

pub(crate) fn group_thread_id(group_did: &str) -> String {
    group_conversation_id(group_did)
}

pub(crate) fn group_conversation_id(group_id_or_did: &str) -> String {
    crate::internal::local_state::owner_scope::group_conversation_id(group_id_or_did)
}

pub(crate) fn mail_conversation_id(source: &str) -> String {
    crate::internal::local_state::owner_scope::mail_conversation_id(source)
}

fn group_storage_key(group_did: &str) -> String {
    group_did.trim().to_owned()
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
fn touch_group_after_outgoing(
    connection: &rusqlite::Connection,
    client: &crate::core::ImClient,
    group_did: &str,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    crate::internal::local_state::groups::upsert_group(
        connection,
        group_touch_record(client, group_did, sdk_result),
    )
}

#[cfg(feature = "sqlite")]
fn direct_outgoing_record(
    client: &crate::core::ImClient,
    target_did: &str,
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    text: &str,
    kind: &crate::messages::MessageKind,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::internal::local_state::messages::MessageRecord {
    let conversation_id = direct_conversation_id_for_scope_or_did(peer_scope, target_did);
    crate::internal::local_state::messages::MessageRecord {
        msg_id: sdk_result.message.id.as_str().to_owned(),
        owner_identity_id: client.current_identity().id.as_str().to_owned(),
        owner_did: client.did().as_str().to_owned(),
        conversation_id: conversation_id.clone(),
        thread_id: conversation_id,
        direction: 1,
        sender_did: client.did().as_str().to_owned(),
        receiver_did: target_did.trim().to_owned(),
        content_type: content_type_for_kind(kind).to_owned(),
        content: text.to_owned(),
        server_seq: sdk_result.message.metadata.server_sequence,
        sent_at: sdk_result.message.sent_at.clone().unwrap_or_default(),
        is_e2ee: false,
        is_read: true,
        metadata: delivery_metadata_json(
            &sdk_result.message.metadata,
            direct_metadata_extras(target_handle, peer_scope, target_did),
        ),
        credential_name: credential_name(client),
        ..crate::internal::local_state::messages::MessageRecord::default()
    }
}

#[cfg(feature = "sqlite")]
fn direct_outgoing_result_record(
    client: &crate::core::ImClient,
    target_did: &str,
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::internal::local_state::messages::MessageRecord {
    let (content_type, content) = body_projection(
        &sdk_result.message.body,
        sdk_result.message.metadata.content_type.as_deref(),
    );
    let conversation_id = direct_conversation_id_for_scope_or_did(peer_scope, target_did);
    crate::internal::local_state::messages::MessageRecord {
        msg_id: sdk_result.message.id.as_str().to_owned(),
        owner_identity_id: client.current_identity().id.as_str().to_owned(),
        owner_did: client.did().as_str().to_owned(),
        conversation_id: conversation_id.clone(),
        thread_id: conversation_id,
        direction: 1,
        sender_did: client.did().as_str().to_owned(),
        receiver_did: target_did.trim().to_owned(),
        content_type,
        content,
        server_seq: sdk_result.message.metadata.server_sequence,
        sent_at: sdk_result.message.sent_at.clone().unwrap_or_default(),
        is_e2ee: false,
        is_read: true,
        metadata: delivery_metadata_json(
            &sdk_result.message.metadata,
            direct_metadata_extras(target_handle, peer_scope, target_did),
        ),
        credential_name: credential_name(client),
        ..crate::internal::local_state::messages::MessageRecord::default()
    }
}

#[cfg(feature = "sqlite")]
fn send_projection_record(
    client: &crate::core::ImClient,
    target: &crate::messages::MessageTarget,
    body: &crate::messages::MessageBody,
    sdk_result: &crate::messages::SendMessageResult,
    target_did: Option<&str>,
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
) -> crate::ImResult<crate::internal::local_state::messages::MessageRecord> {
    let (content_type, content) = request_body_projection(body)?;
    let record = match target {
        crate::messages::MessageTarget::Direct(peer) => {
            let target_did = target_did
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| peer.as_str().trim());
            let conversation_id = direct_conversation_id_for_scope_or_did(peer_scope, target_did);
            crate::internal::local_state::messages::MessageRecord {
                msg_id: sdk_result.message.id.as_str().to_owned(),
                owner_identity_id: client.current_identity().id.as_str().to_owned(),
                owner_did: client.did().as_str().to_owned(),
                conversation_id: conversation_id.clone(),
                thread_id: conversation_id,
                direction: 1,
                sender_did: client.did().as_str().to_owned(),
                receiver_did: target_did.to_owned(),
                content_type,
                content,
                server_seq: sdk_result.message.metadata.server_sequence,
                sent_at: sdk_result.message.sent_at.clone().unwrap_or_default(),
                stored_at: sdk_result
                    .message
                    .sent_at
                    .clone()
                    .unwrap_or_else(crate::internal::wire::common::now_rfc3339),
                is_e2ee: false,
                is_read: true,
                metadata: delivery_metadata_json(
                    &sdk_result.message.metadata,
                    direct_metadata_extras(target_handle, peer_scope, target_did),
                ),
                credential_name: credential_name(client),
                ..crate::internal::local_state::messages::MessageRecord::default()
            }
        }
        crate::messages::MessageTarget::Group(group) => {
            let group_did = group.as_str();
            let conversation_id = group_conversation_id(group_did);
            crate::internal::local_state::messages::MessageRecord {
                msg_id: sdk_result.message.id.as_str().to_owned(),
                owner_identity_id: client.current_identity().id.as_str().to_owned(),
                owner_did: client.did().as_str().to_owned(),
                conversation_id: conversation_id.clone(),
                thread_id: conversation_id,
                direction: 1,
                sender_did: client.did().as_str().to_owned(),
                group_id: group_storage_key(group_did),
                group_did: group_did.trim().to_owned(),
                content_type,
                content,
                server_seq: sdk_result.message.metadata.server_sequence,
                sent_at: sdk_result.message.sent_at.clone().unwrap_or_default(),
                stored_at: sdk_result
                    .message
                    .sent_at
                    .clone()
                    .unwrap_or_else(crate::internal::wire::common::now_rfc3339),
                is_e2ee: false,
                is_read: true,
                metadata: delivery_metadata_json(
                    &sdk_result.message.metadata,
                    Vec::<(&str, String)>::new(),
                ),
                credential_name: credential_name(client),
                ..crate::internal::local_state::messages::MessageRecord::default()
            }
        }
    };
    Ok(record)
}

fn send_projection_result(
    client: &crate::core::ImClient,
    target: &crate::messages::MessageTarget,
    body: &crate::messages::MessageBody,
    message_id: &crate::ids::MessageId,
    operation_id: Option<&str>,
    delivery: crate::messages::DeliveryState,
    target_did: Option<&str>,
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
) -> crate::ImResult<crate::messages::SendMessageResult> {
    let body_view = request_body_view(body)?;
    let now = crate::internal::wire::common::now_rfc3339();
    let retry_target = retry_target_for_body_and_target(body, target)?;
    let operation_id = operation_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let reason = match &delivery {
        crate::messages::DeliveryState::Failed { reason } => {
            Some(reason.trim().to_owned()).filter(|value| !value.is_empty())
        }
        _ => None,
    };
    let send_state_kind = match &delivery {
        crate::messages::DeliveryState::StoredLocally => {
            crate::messages::MessageSendStateKind::Pending
        }
        crate::messages::DeliveryState::Accepted => crate::messages::MessageSendStateKind::Accepted,
        crate::messages::DeliveryState::Sent => crate::messages::MessageSendStateKind::Sent,
        crate::messages::DeliveryState::Failed { .. } => {
            crate::messages::MessageSendStateKind::Failed
        }
    };
    let send_state = crate::messages::MessageSendState {
        state: send_state_kind.clone(),
        operation_id: operation_id.clone(),
        message_id: Some(message_id.clone()),
        reason: reason.clone(),
        updated_at: Some(now.clone()),
    };
    let retry_plan = crate::internal::message_runtime::state::retry_plan_for_state(
        &send_state_kind,
        Some(retry_target),
        operation_id.clone(),
        Some(message_id.clone()),
        reason,
    );
    let thread = send_projection_thread_ref(target, peer_scope)?;
    let conversation_identity = crate::messages::ConversationIdentity::from_thread_ref_for_owner(
        &thread,
        client.did().as_str(),
    );
    let (receiver, group) = match target {
        crate::messages::MessageTarget::Direct(peer) => {
            let receiver = target_did
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| peer.as_str().trim());
            (Some(crate::ids::PeerRef::parse(receiver, "")?), None)
        }
        crate::messages::MessageTarget::Group(group) => (None, Some(group.clone())),
    };
    let mut attributes = Vec::new();
    if let crate::messages::MessageTarget::Direct(peer) = target {
        let target_did = target_did
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| peer.as_str().trim());
        for (key, value) in direct_metadata_extras(target_handle, peer_scope, target_did) {
            attributes.push(crate::messages::MessageMetadataAttribute {
                key: key.to_owned(),
                value,
            });
        }
    }
    let delivery_state =
        crate::internal::message_runtime::state::send_state_label(&send_state_kind).to_owned();
    Ok(crate::messages::SendMessageResult {
        message: crate::messages::Message {
            id: message_id.clone(),
            thread,
            direction: crate::messages::MessageDirection::Outgoing,
            sender: crate::ids::PeerRef::parse(client.did().as_str(), "")?,
            receiver,
            group,
            body: body_view,
            sent_at: Some(now),
            received_at: None,
            metadata: crate::messages::MessageMetadata {
                operation_id,
                delivery_state: Some(delivery_state),
                send_state: Some(send_state),
                retry_plan,
                server_sequence: None,
                content_type: Some(content_type_for_message_body(body)?.to_owned()),
                conversation_identity: Some(conversation_identity),
                attributes,
            },
        },
        delivery,
        warnings: Vec::new(),
    })
}

fn send_projection_thread_ref(
    target: &crate::messages::MessageTarget,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
) -> crate::ImResult<crate::messages::ThreadRef> {
    match target {
        crate::messages::MessageTarget::Direct(peer) => {
            if let Some(scope) = peer_scope {
                return crate::messages::direct_peer_scope_thread_id(
                    scope.user_id.as_str(),
                    scope.full_handle.as_str(),
                )
                .map(crate::messages::ThreadRef::Thread);
            }
            Ok(crate::messages::ThreadRef::Direct(peer.clone()))
        }
        crate::messages::MessageTarget::Group(group) => {
            Ok(crate::messages::ThreadRef::Group(group.clone()))
        }
    }
}

#[cfg(feature = "sqlite")]
fn request_body_projection(
    body: &crate::messages::MessageBody,
) -> crate::ImResult<(String, String)> {
    match body {
        crate::messages::MessageBody::Text { text, kind } => {
            if text.trim().is_empty() {
                return Err(crate::ImError::invalid_input(
                    Some("text".to_owned()),
                    "text message must not be empty",
                ));
            }
            Ok((content_type_for_kind(kind).to_owned(), text.to_owned()))
        }
        crate::messages::MessageBody::Payload { payload } => {
            if !payload.is_object() {
                return Err(crate::ImError::invalid_input(
                    Some("payload".to_owned()),
                    "message payload must be a JSON object",
                ));
            }
            Ok((
                "application/json".to_owned(),
                serde_json::to_string(payload).unwrap_or_default(),
            ))
        }
        crate::messages::MessageBody::Attachment { .. } => {
            Err(crate::ImError::unsupported("conversation-attachment-send"))
        }
    }
}

fn request_body_view(
    body: &crate::messages::MessageBody,
) -> crate::ImResult<crate::messages::MessageBodyView> {
    match body {
        crate::messages::MessageBody::Text { text, kind } => {
            if text.trim().is_empty() {
                return Err(crate::ImError::invalid_input(
                    Some("text".to_owned()),
                    "text message must not be empty",
                ));
            }
            Ok(crate::messages::MessageBodyView::Text {
                text: text.clone(),
                kind: kind.clone(),
            })
        }
        crate::messages::MessageBody::Payload { payload } => {
            if !payload.is_object() {
                return Err(crate::ImError::invalid_input(
                    Some("payload".to_owned()),
                    "message payload must be a JSON object",
                ));
            }
            Ok(crate::messages::MessageBodyView::Payload {
                payload: payload.clone(),
            })
        }
        crate::messages::MessageBody::Attachment { .. } => {
            Err(crate::ImError::unsupported("conversation-attachment-send"))
        }
    }
}

fn retry_target_for_body_and_target(
    body: &crate::messages::MessageBody,
    target: &crate::messages::MessageTarget,
) -> crate::ImResult<crate::internal::message_runtime::state::MessageRetryTarget> {
    match (target, body) {
        (crate::messages::MessageTarget::Direct(_), crate::messages::MessageBody::Text { .. }) => {
            Ok(crate::internal::message_runtime::state::MessageRetryTarget::DirectText)
        }
        (
            crate::messages::MessageTarget::Direct(_),
            crate::messages::MessageBody::Payload { .. },
        ) => Ok(crate::internal::message_runtime::state::MessageRetryTarget::DirectPayload),
        (crate::messages::MessageTarget::Group(_), crate::messages::MessageBody::Text { .. }) => {
            Ok(crate::internal::message_runtime::state::MessageRetryTarget::GroupText)
        }
        (
            crate::messages::MessageTarget::Group(_),
            crate::messages::MessageBody::Payload { .. },
        ) => Ok(crate::internal::message_runtime::state::MessageRetryTarget::GroupPayload),
        (_, crate::messages::MessageBody::Attachment { .. }) => {
            Err(crate::ImError::unsupported("conversation-attachment-send"))
        }
    }
}

fn content_type_for_message_body(
    body: &crate::messages::MessageBody,
) -> crate::ImResult<&'static str> {
    match body {
        crate::messages::MessageBody::Text { kind, .. } => Ok(content_type_for_kind(kind)),
        crate::messages::MessageBody::Payload { .. } => Ok("application/json"),
        crate::messages::MessageBody::Attachment { .. } => {
            Err(crate::ImError::unsupported("conversation-attachment-send"))
        }
    }
}

#[cfg(feature = "sqlite")]
fn direct_e2ee_outgoing_record(
    client: &crate::core::ImClient,
    target_did: &str,
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    text: &str,
    kind: &crate::messages::MessageKind,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::internal::local_state::messages::MessageRecord {
    let conversation_id = direct_conversation_id_for_scope_or_did(peer_scope, target_did);
    crate::internal::local_state::messages::MessageRecord {
        msg_id: sdk_result.message.id.as_str().to_owned(),
        owner_identity_id: client.current_identity().id.as_str().to_owned(),
        owner_did: client.did().as_str().to_owned(),
        conversation_id: conversation_id.clone(),
        thread_id: conversation_id,
        direction: 1,
        sender_did: client.did().as_str().to_owned(),
        receiver_did: target_did.trim().to_owned(),
        content_type: content_type_for_kind(kind).to_owned(),
        content: text.to_owned(),
        server_seq: sdk_result.message.metadata.server_sequence,
        sent_at: sdk_result.message.sent_at.clone().unwrap_or_default(),
        is_e2ee: true,
        is_read: true,
        metadata: secure_metadata_json(
            "direct-e2ee",
            &sdk_result.message.metadata,
            direct_metadata_extras(target_handle, peer_scope, target_did),
        ),
        credential_name: credential_name(client),
        ..crate::internal::local_state::messages::MessageRecord::default()
    }
}

#[cfg(feature = "sqlite")]
fn direct_e2ee_attachment_outgoing_record(
    client: &crate::core::ImClient,
    target_did: &str,
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    redacted_manifest: &Value,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::internal::local_state::messages::MessageRecord {
    let conversation_id = direct_conversation_id_for_scope_or_did(peer_scope, target_did);
    crate::internal::local_state::messages::MessageRecord {
        msg_id: sdk_result.message.id.as_str().to_owned(),
        owner_identity_id: client.current_identity().id.as_str().to_owned(),
        owner_did: client.did().as_str().to_owned(),
        conversation_id: conversation_id.clone(),
        thread_id: conversation_id,
        direction: 1,
        sender_did: client.did().as_str().to_owned(),
        receiver_did: target_did.trim().to_owned(),
        content_type: crate::attachments::manifest::attachment_manifest_content_type().to_owned(),
        content: crate::attachments::manifest::manifest_content_string(redacted_manifest),
        server_seq: sdk_result.message.metadata.server_sequence,
        sent_at: sdk_result.message.sent_at.clone().unwrap_or_default(),
        is_e2ee: true,
        is_read: true,
        metadata: secure_metadata_json(
            "direct-e2ee",
            &sdk_result.message.metadata,
            attachment_metadata_extras(redacted_manifest, target_handle, peer_scope, target_did),
        ),
        credential_name: credential_name(client),
        ..crate::internal::local_state::messages::MessageRecord::default()
    }
}

#[cfg(feature = "sqlite")]
fn group_outgoing_record(
    client: &crate::core::ImClient,
    group_did: &str,
    text: &str,
    kind: &crate::messages::MessageKind,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::internal::local_state::messages::MessageRecord {
    let conversation_id = group_conversation_id(group_did);
    crate::internal::local_state::messages::MessageRecord {
        msg_id: sdk_result.message.id.as_str().to_owned(),
        owner_identity_id: client.current_identity().id.as_str().to_owned(),
        owner_did: client.did().as_str().to_owned(),
        conversation_id: conversation_id.clone(),
        thread_id: conversation_id,
        direction: 1,
        sender_did: client.did().as_str().to_owned(),
        group_id: group_storage_key(group_did),
        group_did: group_did.trim().to_owned(),
        content_type: content_type_for_kind(kind).to_owned(),
        content: text.to_owned(),
        server_seq: sdk_result.message.metadata.server_sequence,
        sent_at: sdk_result.message.sent_at.clone().unwrap_or_default(),
        is_e2ee: false,
        is_read: true,
        metadata: delivery_metadata_json(
            &sdk_result.message.metadata,
            Vec::<(&str, String)>::new(),
        ),
        credential_name: credential_name(client),
        ..crate::internal::local_state::messages::MessageRecord::default()
    }
}

#[cfg(feature = "sqlite")]
fn group_outgoing_result_record(
    client: &crate::core::ImClient,
    group_did: &str,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::internal::local_state::messages::MessageRecord {
    let (content_type, content) = body_projection(
        &sdk_result.message.body,
        sdk_result.message.metadata.content_type.as_deref(),
    );
    let conversation_id = group_conversation_id(group_did);
    crate::internal::local_state::messages::MessageRecord {
        msg_id: sdk_result.message.id.as_str().to_owned(),
        owner_identity_id: client.current_identity().id.as_str().to_owned(),
        owner_did: client.did().as_str().to_owned(),
        conversation_id: conversation_id.clone(),
        thread_id: conversation_id,
        direction: 1,
        sender_did: client.did().as_str().to_owned(),
        group_id: group_storage_key(group_did),
        group_did: group_did.trim().to_owned(),
        content_type,
        content,
        server_seq: sdk_result.message.metadata.server_sequence,
        sent_at: sdk_result.message.sent_at.clone().unwrap_or_default(),
        is_e2ee: false,
        is_read: true,
        metadata: delivery_metadata_json(
            &sdk_result.message.metadata,
            Vec::<(&str, String)>::new(),
        ),
        credential_name: credential_name(client),
        ..crate::internal::local_state::messages::MessageRecord::default()
    }
}

#[cfg(feature = "sqlite")]
fn direct_attachment_outgoing_record(
    client: &crate::core::ImClient,
    target_did: &str,
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    manifest: &Value,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::internal::local_state::messages::MessageRecord {
    let conversation_id = direct_conversation_id_for_scope_or_did(peer_scope, target_did);
    crate::internal::local_state::messages::MessageRecord {
        msg_id: sdk_result.message.id.as_str().to_owned(),
        owner_identity_id: client.current_identity().id.as_str().to_owned(),
        owner_did: client.did().as_str().to_owned(),
        conversation_id: conversation_id.clone(),
        thread_id: conversation_id,
        direction: 1,
        sender_did: client.did().as_str().to_owned(),
        receiver_did: target_did.trim().to_owned(),
        content_type: crate::attachments::manifest::attachment_manifest_content_type().to_owned(),
        content: crate::attachments::manifest::manifest_content_string(manifest),
        server_seq: sdk_result.message.metadata.server_sequence,
        sent_at: sdk_result.message.sent_at.clone().unwrap_or_default(),
        is_e2ee: false,
        is_read: true,
        metadata: delivery_metadata_json(
            &sdk_result.message.metadata,
            attachment_metadata_extras(manifest, target_handle, peer_scope, target_did),
        ),
        credential_name: credential_name(client),
        ..crate::internal::local_state::messages::MessageRecord::default()
    }
}

#[cfg(feature = "sqlite")]
fn group_attachment_outgoing_record(
    client: &crate::core::ImClient,
    group_did: &str,
    manifest: &Value,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::internal::local_state::messages::MessageRecord {
    let conversation_id = group_conversation_id(group_did);
    crate::internal::local_state::messages::MessageRecord {
        msg_id: sdk_result.message.id.as_str().to_owned(),
        owner_identity_id: client.current_identity().id.as_str().to_owned(),
        owner_did: client.did().as_str().to_owned(),
        conversation_id: conversation_id.clone(),
        thread_id: conversation_id,
        direction: 1,
        sender_did: client.did().as_str().to_owned(),
        group_id: group_storage_key(group_did),
        group_did: group_did.trim().to_owned(),
        content_type: crate::attachments::manifest::attachment_manifest_content_type().to_owned(),
        content: crate::attachments::manifest::manifest_content_string(manifest),
        server_seq: sdk_result.message.metadata.server_sequence,
        sent_at: sdk_result.message.sent_at.clone().unwrap_or_default(),
        is_e2ee: false,
        is_read: true,
        metadata: delivery_metadata_json(
            &sdk_result.message.metadata,
            attachment_metadata_extras(manifest, None, None, ""),
        ),
        credential_name: credential_name(client),
        ..crate::internal::local_state::messages::MessageRecord::default()
    }
}

#[cfg(feature = "sqlite")]
fn group_touch_record(
    client: &crate::core::ImClient,
    group_did: &str,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::internal::local_state::groups::GroupRecord {
    crate::internal::local_state::groups::GroupRecord {
        owner_identity_id: client.current_identity().id.as_str().to_owned(),
        owner_did: client.did().as_str().to_owned(),
        group_id: group_storage_key(group_did),
        group_did: group_did.trim().to_owned(),
        membership_status: "active".to_owned(),
        last_synced_seq: sdk_result.message.metadata.server_sequence,
        last_message_at: sdk_result.message.sent_at.clone().unwrap_or_default(),
        metadata: delivery_metadata_json(
            &sdk_result.message.metadata,
            Vec::<(&str, String)>::new(),
        ),
        credential_name: credential_name(client),
        ..crate::internal::local_state::groups::GroupRecord::default()
    }
}

fn delivery_metadata_json<'a, I>(metadata: &crate::messages::MessageMetadata, extras: I) -> String
where
    I: IntoIterator<Item = (&'a str, String)>,
{
    let mut object = Map::new();
    insert_string(
        &mut object,
        "operation_id",
        metadata.operation_id.as_deref(),
    );
    insert_string(
        &mut object,
        "delivery_state",
        metadata.delivery_state.as_deref(),
    );
    insert_string(
        &mut object,
        "content_type",
        metadata.content_type.as_deref(),
    );
    if let Some(server_sequence) = metadata.server_sequence {
        object.insert(
            "server_sequence".to_owned(),
            Value::Number(server_sequence.into()),
        );
        object.insert(
            "group_event_seq".to_owned(),
            Value::String(server_sequence.to_string()),
        );
    }
    if let Some(send_state) = metadata.send_state.as_ref() {
        object.insert(
            "send_state".to_owned(),
            serde_json::to_value(send_state).unwrap_or(Value::Null),
        );
    }
    if let Some(retry_plan) = metadata.retry_plan.as_ref() {
        object.insert(
            "retry_plan".to_owned(),
            serde_json::to_value(retry_plan).unwrap_or(Value::Null),
        );
    }
    for attribute in &metadata.attributes {
        match attribute.key.as_str() {
            "raw_message_id"
            | "group_event_seq"
            | "group_state_version"
            | "attachment_id"
            | "object_uri"
            | "target_handle"
            | "resolved_target_did"
            | "peer_user_id"
            | "peer_full_handle"
            | "peer_current_did"
            | "decryption_state"
            | "secure_wire_content_type"
                if !attribute.value.trim().is_empty() =>
            {
                object.insert(
                    attribute.key.clone(),
                    Value::String(attribute.value.clone()),
                );
            }
            "attachment_manifest" => {
                insert_attachment_manifest_fields(&mut object, &attribute.value);
            }
            _ => {}
        }
    }
    for (key, value) in extras {
        insert_string(&mut object, key, Some(value.as_str()));
    }
    Value::Object(object).to_string()
}

fn direct_metadata_extras<'a>(
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    target_did: &str,
) -> Vec<(&'a str, String)> {
    let mut extras = Vec::new();
    if let Some(target_handle) = target_handle
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        extras.push(("target_handle", target_handle.to_owned()));
    }
    if let Some(scope) = peer_scope {
        extras.push(("peer_user_id", scope.user_id.clone()));
        extras.push(("peer_full_handle", scope.full_handle.clone()));
    }
    let target_did = target_did.trim();
    if !target_did.is_empty() {
        extras.push(("resolved_target_did", target_did.to_owned()));
        extras.push(("peer_current_did", target_did.to_owned()));
    }
    extras
}

fn attachment_metadata_extras<'a>(
    manifest: &'a Value,
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    target_did: &str,
) -> Vec<(&'a str, String)> {
    let mut extras = direct_metadata_extras(target_handle, peer_scope, target_did);
    if let Some(caption) = manifest
        .get("caption")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        extras.push(("caption", caption.to_owned()));
    }
    if let Some(attachment) = manifest
        .get("attachments")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
    {
        if let Some(attachment_id) = attachment
            .get("attachment_id")
            .or_else(|| attachment.get("id"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            extras.push(("attachment_id", attachment_id.to_owned()));
        }
        if let Some(object_uri) = attachment
            .get("object_uri")
            .or_else(|| {
                attachment
                    .get("access_info")
                    .and_then(|value| value.get("object_uri"))
            })
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            extras.push(("object_uri", object_uri.to_owned()));
        }
    }
    extras
}

fn insert_attachment_manifest_fields(object: &mut Map<String, Value>, manifest: &str) {
    let Ok(value) = serde_json::from_str::<Value>(manifest) else {
        return;
    };
    if let Some(caption) = value
        .get("caption")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        object.insert("caption".to_owned(), Value::String(caption.to_owned()));
    }
    let Some(attachment) = value
        .get("attachments")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
    else {
        return;
    };
    if let Some(attachment_id) = attachment
        .get("attachment_id")
        .or_else(|| attachment.get("id"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        object.insert(
            "attachment_id".to_owned(),
            Value::String(attachment_id.to_owned()),
        );
    }
    if let Some(object_uri) = attachment
        .get("object_uri")
        .or_else(|| {
            attachment
                .get("access_info")
                .and_then(|value| value.get("object_uri"))
        })
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        object.insert(
            "object_uri".to_owned(),
            Value::String(object_uri.to_owned()),
        );
    }
}

fn secure_metadata_json<'a, I>(
    security: &str,
    metadata: &crate::messages::MessageMetadata,
    extras: I,
) -> String
where
    I: IntoIterator<Item = (&'a str, String)>,
{
    let mut object = Map::new();
    object.insert("security".to_owned(), Value::String(security.to_owned()));
    object.insert(
        "redaction_version".to_owned(),
        Value::String("v1".to_owned()),
    );
    object.insert("contains_sensitive".to_owned(), Value::Bool(false));
    insert_string(
        &mut object,
        "operation_id",
        metadata.operation_id.as_deref(),
    );
    insert_string(
        &mut object,
        "delivery_state",
        metadata.delivery_state.as_deref(),
    );
    insert_string(
        &mut object,
        "content_type",
        metadata.content_type.as_deref(),
    );
    if let Some(server_sequence) = metadata.server_sequence {
        object.insert(
            "server_sequence".to_owned(),
            Value::Number(server_sequence.into()),
        );
    }
    if let Some(send_state) = metadata.send_state.as_ref() {
        object.insert(
            "send_state".to_owned(),
            serde_json::to_value(send_state).unwrap_or(Value::Null),
        );
    }
    if let Some(retry_plan) = metadata.retry_plan.as_ref() {
        object.insert(
            "retry_plan".to_owned(),
            serde_json::to_value(retry_plan).unwrap_or(Value::Null),
        );
    }
    for attribute in &metadata.attributes {
        match attribute.key.as_str() {
            "raw_message_id"
            | "group_event_seq"
            | "group_state_version"
            | "secure_outbox_id"
            | "target_handle"
            | "resolved_target_did"
            | "peer_user_id"
            | "peer_full_handle"
            | "peer_current_did"
            | "decryption_state"
            | "secure_wire_content_type"
                if !attribute.value.trim().is_empty() =>
            {
                object.insert(
                    attribute.key.clone(),
                    Value::String(attribute.value.clone()),
                );
            }
            "security" => {}
            "attachment_manifest" => {
                insert_attachment_manifest_fields(&mut object, &attribute.value);
            }
            _ => {}
        }
    }
    for (key, value) in extras {
        insert_string(&mut object, key, Some(value.as_str()));
    }
    Value::Object(object).to_string()
}

fn secure_metadata_json_without_extras(
    security: &str,
    metadata: &crate::messages::MessageMetadata,
) -> String {
    secure_metadata_json(security, metadata, std::iter::empty::<(&str, String)>())
}

fn insert_string(object: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    object.insert(key.to_owned(), Value::String(value.to_owned()));
}

fn credential_name(client: &crate::core::ImClient) -> String {
    client.current_identity().id.as_str().to_owned()
}

#[cfg(feature = "sqlite")]
pub(crate) fn message_record_from_message(
    client: &crate::core::ImClient,
    message: &crate::messages::Message,
) -> crate::ImResult<crate::internal::local_state::messages::MessageRecord> {
    let (content_type, content) =
        body_projection(&message.body, message.metadata.content_type.as_deref());
    let direction = direction_value_for_message(client.did().as_str(), message);
    let conversation_id = conversation_id_for_message(client.did().as_str(), message);
    Ok(crate::internal::local_state::messages::MessageRecord {
        msg_id: message.id.as_str().to_owned(),
        owner_identity_id: client.current_identity().id.as_str().to_owned(),
        owner_did: client.did().as_str().to_owned(),
        conversation_id: conversation_id.clone(),
        thread_id: conversation_id,
        direction,
        sender_did: message.sender.as_str().to_owned(),
        receiver_did: message
            .receiver
            .as_ref()
            .map(|receiver| receiver.as_str().to_owned())
            .unwrap_or_default(),
        group_id: group_ref_for_message(message).unwrap_or_default(),
        group_did: group_ref_for_message(message).unwrap_or_default(),
        content_type,
        content,
        server_seq: message.metadata.server_sequence,
        sent_at: message.sent_at.clone().unwrap_or_default(),
        stored_at: message
            .received_at
            .clone()
            .or_else(|| message.sent_at.clone())
            .unwrap_or_default(),
        is_e2ee: is_e2ee_message(message),
        is_read: read_state_for_message(direction, message),
        metadata: read_metadata_json(&message.metadata),
        credential_name: credential_name(client),
        ..crate::internal::local_state::messages::MessageRecord::default()
    })
}

#[cfg(feature = "sqlite")]
fn conversation_id_for_message(owner_did: &str, message: &crate::messages::Message) -> String {
    if let Some(group) = group_ref_for_message(message) {
        return group_conversation_id(&group);
    }
    if let Some(scope) = peer_scope_from_metadata(&message.metadata) {
        return direct_conversation_id_for_peer_scope(&scope);
    }
    let peer = direct_peer_for_message(owner_did, message);
    if !peer.trim().is_empty() {
        return direct_conversation_id(&peer);
    }
    match &message.thread {
        crate::messages::ThreadRef::Thread(thread) => thread.as_str().to_owned(),
        crate::messages::ThreadRef::Direct(peer) => direct_conversation_id(peer.as_str()),
        crate::messages::ThreadRef::Group(group) => group_conversation_id(group.as_str()),
    }
}

pub(crate) fn peer_scope_from_metadata(
    metadata: &crate::messages::MessageMetadata,
) -> Option<crate::internal::local_state::owner_scope::DirectPeerScope> {
    let user_id = metadata_string_attribute(metadata, "peer_user_id")?;
    let full_handle = metadata_string_attribute(metadata, "peer_full_handle")?;
    crate::internal::local_state::owner_scope::DirectPeerScope::new(user_id, full_handle).ok()
}

fn metadata_string_attribute(
    metadata: &crate::messages::MessageMetadata,
    key: &str,
) -> Option<String> {
    metadata
        .attributes
        .iter()
        .find(|attribute| attribute.key == key)
        .map(|attribute| attribute.value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(feature = "sqlite")]
fn direct_peer_for_message(owner_did: &str, message: &crate::messages::Message) -> String {
    if message.sender.as_str() != owner_did {
        return message.sender.as_str().to_owned();
    }
    if let Some(receiver) = message.receiver.as_ref() {
        return receiver.as_str().to_owned();
    }
    match &message.thread {
        crate::messages::ThreadRef::Direct(peer) => peer.as_str().to_owned(),
        _ => String::new(),
    }
}

#[cfg(feature = "sqlite")]
fn group_ref_for_message(message: &crate::messages::Message) -> Option<String> {
    message
        .group
        .as_ref()
        .map(|group| group.as_str().to_owned())
        .or_else(|| match &message.thread {
            crate::messages::ThreadRef::Group(group) => Some(group.as_str().to_owned()),
            _ => None,
        })
        .filter(|group| !group.trim().is_empty())
}

#[cfg(feature = "sqlite")]
fn direction_value_for_message(owner_did: &str, message: &crate::messages::Message) -> i64 {
    match message.direction {
        crate::messages::MessageDirection::Incoming => 0,
        crate::messages::MessageDirection::Outgoing => 1,
        crate::messages::MessageDirection::Unknown => {
            if message.sender.as_str() == owner_did {
                1
            } else if !message.sender.as_str().trim().is_empty() {
                0
            } else {
                -1
            }
        }
    }
}

#[cfg(feature = "sqlite")]
fn read_state_for_message(direction: i64, message: &crate::messages::Message) -> bool {
    match metadata_bool_attribute(&message.metadata, "is_read") {
        Some(value) => value,
        None => direction != 0,
    }
}

#[cfg(feature = "sqlite")]
fn metadata_bool_attribute(metadata: &crate::messages::MessageMetadata, key: &str) -> Option<bool> {
    metadata
        .attributes
        .iter()
        .find(|attribute| attribute.key == key)
        .and_then(|attribute| match attribute.value.trim() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        })
}

#[cfg(feature = "sqlite")]
fn body_projection(
    body: &crate::messages::MessageBodyView,
    metadata_content_type: Option<&str>,
) -> (String, String) {
    match body {
        crate::messages::MessageBodyView::Text { text, kind } => {
            (content_type_for_kind(kind).to_owned(), text.to_owned())
        }
        crate::messages::MessageBodyView::Payload { payload } => (
            payload_content_type(metadata_content_type),
            serde_json::to_string(payload).unwrap_or_default(),
        ),
        crate::messages::MessageBodyView::Unsupported { content_type } => {
            (content_type.clone().unwrap_or_default(), String::new())
        }
    }
}

#[cfg(feature = "sqlite")]
fn payload_content_type(metadata_content_type: Option<&str>) -> String {
    let Some(content_type) = metadata_content_type
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return "application/json".to_owned();
    };
    if content_type == crate::attachments::manifest::attachment_manifest_content_type() {
        return content_type.to_owned();
    }
    "application/json".to_owned()
}

#[cfg(feature = "sqlite")]
fn is_e2ee_message(message: &crate::messages::Message) -> bool {
    message
        .metadata
        .content_type
        .as_deref()
        .map(|content_type| content_type.contains("cipher") || content_type.contains("e2ee"))
        .unwrap_or(false)
        || message
            .metadata
            .attributes
            .iter()
            .any(|attribute| attribute.key == "security" && attribute.value.contains("e2ee"))
}

#[cfg(feature = "sqlite")]
fn read_metadata_json(metadata: &crate::messages::MessageMetadata) -> String {
    let mut object = Map::new();
    insert_string(
        &mut object,
        "operation_id",
        metadata.operation_id.as_deref(),
    );
    insert_string(
        &mut object,
        "delivery_state",
        metadata.delivery_state.as_deref(),
    );
    insert_string(
        &mut object,
        "content_type",
        metadata.content_type.as_deref(),
    );
    if let Some(server_sequence) = metadata.server_sequence {
        object.insert(
            "server_sequence".to_owned(),
            Value::Number(server_sequence.into()),
        );
    }
    if let Some(send_state) = metadata.send_state.as_ref() {
        object.insert(
            "send_state".to_owned(),
            serde_json::to_value(send_state).unwrap_or(Value::Null),
        );
    }
    if let Some(retry_plan) = metadata.retry_plan.as_ref() {
        object.insert(
            "retry_plan".to_owned(),
            serde_json::to_value(retry_plan).unwrap_or(Value::Null),
        );
    }
    for attribute in &metadata.attributes {
        match attribute.key.as_str() {
            "raw_message_id"
            | "group_event_seq"
            | "is_read"
            | "senderName"
            | "sender_name"
            | "target_handle"
            | "resolved_target_did"
            | "peer_user_id"
            | "peer_full_handle"
            | "peer_current_did"
            | "decryption_state"
            | "secure_wire_content_type"
                if !attribute.value.trim().is_empty() =>
            {
                object.insert(
                    attribute.key.clone(),
                    Value::String(attribute.value.clone()),
                );
            }
            _ => {}
        }
    }
    Value::Object(object).to_string()
}

fn content_type_for_kind(kind: &crate::messages::MessageKind) -> &'static str {
    match kind {
        crate::messages::MessageKind::Markdown => "text/markdown",
        crate::messages::MessageKind::Text => "text/plain",
    }
}

fn normalize_handle_value(value: &str) -> String {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        return String::new();
    }
    let value = value.trim_start_matches("wba://");
    match value.find('.') {
        Some(index) if index > 0 => value[..index].to_owned(),
        _ => value.to_owned(),
    }
}

fn merge_peer_dids(current: &str, historical: &[String]) -> Vec<String> {
    let mut seen = Vec::with_capacity(historical.len() + 1);
    let mut result = Vec::with_capacity(historical.len() + 1);
    let current = current.trim();
    if !current.is_empty() {
        seen.push(current.to_owned());
        result.push(current.to_owned());
    }
    for did in historical {
        let did = did.trim();
        if did.is_empty() || seen.iter().any(|known| known == did) {
            continue;
        }
        seen.push(did.to_owned());
        result.push(did.to_owned());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_ids_do_not_include_local_owner_did() {
        assert_eq!(
            direct_conversation_id("did:example:bob"),
            "dm:did:example:bob"
        );
        assert_eq!(
            group_conversation_id("did:example:group"),
            "group:did:example:group"
        );
        assert_eq!(mail_conversation_id("inbox"), "mail:inbox");
    }

    #[test]
    fn secure_metadata_keeps_only_redacted_delivery_fields() {
        let metadata = crate::messages::MessageMetadata {
            operation_id: Some("op-1".to_owned()),
            delivery_state: Some("accepted".to_owned()),
            send_state: Some(crate::messages::MessageSendState {
                state: crate::messages::MessageSendStateKind::Accepted,
                operation_id: Some("op-1".to_owned()),
                message_id: Some(crate::ids::MessageId::parse("msg-1").unwrap()),
                reason: None,
                updated_at: Some("2026-05-24T00:00:00Z".to_owned()),
            }),
            server_sequence: Some(7),
            content_type: Some("text/plain".to_owned()),
            attributes: vec![
                crate::messages::MessageMetadataAttribute {
                    key: "group_event_seq".to_owned(),
                    value: "7".to_owned(),
                },
                crate::messages::MessageMetadataAttribute {
                    key: "private_message_b64u".to_owned(),
                    value: "cipher".to_owned(),
                },
            ],
            ..Default::default()
        };

        let encoded = secure_metadata_json_without_extras("group-e2ee", &metadata);
        let value: Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(value["security"], "group-e2ee");
        assert_eq!(value["contains_sensitive"], false);
        assert_eq!(value["group_event_seq"], "7");
        assert!(value.get("private_message_b64u").is_none());
        assert!(!encoded.contains("cipher"));
    }

    #[test]
    fn payload_body_projection_uses_application_json_without_stringifying_as_text() {
        let payload = serde_json::json!({
            "schema": "awiki.agent.status.v1",
            "state": "running"
        });
        let (content_type, content) = body_projection(
            &crate::messages::MessageBodyView::Payload {
                payload: payload.clone(),
            },
            None,
        );

        assert_eq!(content_type, "application/json");
        let stored: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(stored, payload);
    }

    #[test]
    fn payload_body_projection_preserves_attachment_manifest_content_type() {
        let payload = serde_json::json!({
            "attachments": [{
                "attachment_id": "att-1",
                "filename": "report.md",
                "mime_type": "text/markdown",
                "size": "12"
            }],
            "primary_attachment_id": "att-1"
        });
        let (content_type, content) = body_projection(
            &crate::messages::MessageBodyView::Payload {
                payload: payload.clone(),
            },
            Some(crate::attachments::manifest::attachment_manifest_content_type()),
        );

        assert_eq!(
            content_type,
            crate::attachments::manifest::attachment_manifest_content_type()
        );
        let stored: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(stored, payload);
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn conversation_send_projection_persists_pending_and_failed_state() {
        let fixture = Fixture::new("conversation-send-projection");
        let client = fixture.client();
        let target = crate::messages::MessageTarget::Direct(
            crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
        );
        let body = crate::messages::MessageBody::Text {
            text: "hello durable pending".to_owned(),
            kind: crate::messages::MessageKind::Text,
        };
        let message_id = crate::ids::MessageId::parse("msg-durable-pending").unwrap();

        persist_send_projection_async(
            &client,
            &target,
            &body,
            &message_id,
            Some("op-durable-pending"),
            crate::messages::DeliveryState::StoredLocally,
            Some("did:example:bob"),
            None,
            None,
        )
        .await
        .unwrap();

        let page = client
            .messages()
            .local_conversation_timeline_with_metadata_async(
                crate::messages::ConversationReadRef::new("dm:did:example:bob").unwrap(),
                crate::messages::LocalHistoryQuery {
                    limit: crate::ids::PageLimit(20),
                    cursor: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(page.items.len(), 1);
        let pending = &page.items[0];
        assert_eq!(pending.id.as_str(), "msg-durable-pending");
        assert_eq!(
            pending.metadata.send_state.as_ref().unwrap().state,
            crate::messages::MessageSendStateKind::Pending
        );
        assert!(pending.metadata.retry_plan.is_none());

        persist_send_projection_async(
            &client,
            &target,
            &body,
            &message_id,
            Some("op-durable-pending"),
            crate::messages::DeliveryState::Failed {
                reason: "network unavailable".to_owned(),
            },
            Some("did:example:bob"),
            None,
            None,
        )
        .await
        .unwrap();

        let page = client
            .messages()
            .local_conversation_timeline_with_metadata_async(
                crate::messages::ConversationReadRef::new("dm:did:example:bob").unwrap(),
                crate::messages::LocalHistoryQuery {
                    limit: crate::ids::PageLimit(20),
                    cursor: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(page.items.len(), 1, "failure must update the same row");
        let failed = &page.items[0];
        let send_state = failed.metadata.send_state.as_ref().unwrap();
        assert_eq!(
            send_state.state,
            crate::messages::MessageSendStateKind::Failed
        );
        assert_eq!(send_state.reason.as_deref(), Some("network unavailable"));
        let retry_plan = failed.metadata.retry_plan.as_ref().unwrap();
        assert!(retry_plan.retryable);
        assert_eq!(
            retry_plan.action,
            crate::messages::MessageRetryAction::RetryDirectText
        );
        assert_eq!(
            retry_plan.message_id.as_ref().unwrap().as_str(),
            "msg-durable-pending"
        );
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn conversation_send_projection_uses_peer_scope_conversation_id() {
        let fixture = Fixture::new("conversation-send-peer-scope");
        let client = fixture.client();
        let scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
            "user-bob",
            "bob.awiki.test",
        )
        .unwrap();
        let conversation_id =
            crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
                &scope,
            );
        let target = crate::messages::MessageTarget::Direct(
            crate::ids::PeerRef::parse("bob.awiki.test", "").unwrap(),
        );
        let body = crate::messages::MessageBody::Payload {
            payload: serde_json::json!({
                "schema": "awiki.agent.mention.v1",
                "text": "@Bob hello"
            }),
        };

        persist_send_projection_async(
            &client,
            &target,
            &body,
            &crate::ids::MessageId::parse("msg-peer-scope-pending").unwrap(),
            Some("op-peer-scope-pending"),
            crate::messages::DeliveryState::StoredLocally,
            Some("did:example:bob-current"),
            Some("bob.awiki.test"),
            Some(&scope),
        )
        .await
        .unwrap();

        let page = client
            .messages()
            .local_conversation_timeline_with_metadata_async(
                crate::messages::ConversationReadRef::new(conversation_id.clone()).unwrap(),
                crate::messages::LocalHistoryQuery {
                    limit: crate::ids::PageLimit(20),
                    cursor: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(page.items.len(), 1);
        let message = &page.items[0];
        assert_eq!(
            message
                .metadata
                .conversation_identity
                .as_ref()
                .unwrap()
                .conversation_id,
            conversation_id
        );
        assert!(message
            .metadata
            .attributes
            .iter()
            .any(|attribute| { attribute.key == "peer_user_id" && attribute.value == "user-bob" }));
        assert!(message.metadata.attributes.iter().any(|attribute| {
            attribute.key == "peer_current_did" && attribute.value == "did:example:bob-current"
        }));
    }

    #[cfg(feature = "sqlite")]
    struct Fixture {
        root: std::path::PathBuf,
    }

    #[cfg(feature = "sqlite")]
    impl Fixture {
        fn new(prefix: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir()
                .join(format!("im-core-{prefix}-{}-{nanos}", std::process::id()));
            let identity_root = root.join("identities");
            let identity_dir = identity_root.join("alice");
            std::fs::create_dir_all(&identity_dir).unwrap();
            std::fs::create_dir_all(root.join("local")).unwrap();
            std::fs::write(identity_root.join("default"), "alice\n").unwrap();
            std::fs::write(
                identity_root.join("registry.json"),
                serde_json::json!({
                    "default_identity": "alice",
                    "identities": [{
                        "id": "alice-id",
                        "did": "did:example:alice",
                        "local_alias": "alice",
                        "ready_for_auth": true,
                        "ready_for_messaging": true,
                        "missing": []
                    }]
                })
                .to_string(),
            )
            .unwrap();
            std::fs::write(identity_dir.join("did.json"), "{}").unwrap();
            Self { root }
        }

        fn client(&self) -> crate::core::ImClient {
            crate::core::ImCore::new(
                crate::ImCoreConfig {
                    service_base_url: crate::ServiceEndpoint::parse("https://example.test")
                        .unwrap(),
                    did_domain: "awiki.test".to_owned(),
                    user_service_endpoint: None,
                    message_service_endpoint: None,
                    mail_service_endpoint: None,
                    anp_service_endpoint: None,
                    anp_service_did: None,
                    ca_bundle: None,
                    transport_policy: crate::MessageTransportPolicy::HttpOnly,
                },
                crate::ImCorePaths {
                    identities: crate::IdentityRegistryPaths {
                        identity_root_dir: self.root.join("identities"),
                        registry_path: self.root.join("identities").join("registry.json"),
                        default_identity_path: Some(self.root.join("identities").join("default")),
                    },
                    local_state: crate::LocalStatePaths {
                        sqlite_path: self.root.join("local").join("im.sqlite"),
                    },
                    runtime: crate::RuntimePaths {
                        cache_dir: self.root.join("cache"),
                        temp_dir: self.root.join("tmp"),
                    },
                },
            )
            .unwrap()
            .client(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_owned(),
            ))
            .unwrap()
        }
    }
}
