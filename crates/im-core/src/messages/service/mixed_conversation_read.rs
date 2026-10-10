use crate::internal::local_state::{self, messages::mixed_read::Plan};
use crate::messages::{MarkConversationReadRequest, MarkThreadReadResult};

pub(super) fn prepare(
    client: &crate::core::ImClient,
    request: &MarkConversationReadRequest,
) -> crate::ImResult<Option<Plan>> {
    let db = local_state::open_writable(&client.core_inner().sdk_paths().local_state.sqlite_path)?;
    local_state::messages::mixed_read::prepare(
        &db,
        client.current_identity().id.as_str(),
        &request.conversation.conversation_id,
        request.watermark.as_ref(),
    )
}

fn local_result() -> MarkThreadReadResult {
    MarkThreadReadResult {
        updated_count: 0,
        remote_acknowledged: false,
        partial: false,
        fallback_used: false,
        pending_remote_ack: false,
        effective_watermark: None,
        legacy_message_ids: Vec::new(),
        warnings: Vec::new(),
    }
}

fn finish(
    client: &crate::core::ImClient,
    plan: Plan,
    mut result: MarkThreadReadResult,
) -> crate::ImResult<MarkThreadReadResult> {
    if !plan.local_ids.is_empty() {
        let mut db =
            local_state::open_writable(&client.core_inner().sdk_paths().local_state.sqlite_path)?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(local_state::local_state_unavailable)?;
        let updated = local_state::messages::mark_messages_read_for_owner_identity(
            &tx,
            client.current_identity().id.as_str(),
            client.did().as_str(),
            &plan.local_ids,
        )?;
        tx.commit().map_err(local_state::local_state_unavailable)?;
        result.updated_count = result
            .updated_count
            .saturating_add(u32::try_from(updated).unwrap_or(u32::MAX));
        if updated > 0 {
            client.emit_committed_conversation_projection("local_mark_conversation_p5_read");
        }
    }
    result.effective_watermark = Some(plan.effective);
    Ok(result)
}

#[cfg(feature = "blocking")]
pub(super) fn commit(
    client: &crate::core::ImClient,
    mut request: MarkConversationReadRequest,
    plan: Plan,
) -> crate::ImResult<MarkThreadReadResult> {
    let result = if let Some(watermark) = plan.ordinary.clone() {
        request.watermark = Some(watermark);
        let mapped = super::mark_read_input_for_conversation(client, request)?;
        crate::internal::message_runtime::mark_read::MessageMarkReadRuntime::new(
            client,
            crate::internal::auth::session::FileSessionProvider::new(client),
            crate::internal::transport::CoreHttpTransport::new(client),
        )
        .mark_thread_read(mapped)?
        .sdk_result
    } else {
        local_result()
    };
    finish(client, plan, result)
}

pub(super) async fn commit_async(
    client: &crate::core::ImClient,
    mut request: MarkConversationReadRequest,
    plan: Plan,
) -> crate::ImResult<MarkThreadReadResult> {
    let result = if let Some(watermark) = plan.ordinary.clone() {
        request.watermark = Some(watermark);
        let mapped = super::mark_read_input_for_conversation_async(client, request).await?;
        crate::internal::message_runtime::mark_read::MessageMarkReadRuntime::new(
            client,
            crate::internal::auth::session::FileSessionProvider::new(client),
            crate::internal::transport::CoreHttpTransport::new(client),
        )
        .mark_thread_read_async(mapped)
        .await?
        .sdk_result
    } else {
        local_result()
    };
    finish(client, plan, result)
}
