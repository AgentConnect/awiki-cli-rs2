use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use napi_derive::napi;
use tokio::sync::{Notify, RwLock, RwLockReadGuard, RwLockWriteGuard};
use tokio_util::sync::CancellationToken;

use crate::dto::*;
use crate::error::{napi_result, SafeError, SafeResult};
use crate::state::StateRoot;

const LIFECYCLE_OPEN: u8 = 0;
const LIFECYCLE_CLOSING: u8 = 1;
const LIFECYCLE_CLOSED: u8 = 2;
const DEFAULT_OPERATION_TIMEOUT_MS: u32 = 120_000;
const DEFAULT_SYNC_TIMEOUT_MS: u32 = 15_000;
const MAX_TIMEOUT_MS: u32 = 600_000;

struct Environment {
    core: im_core::ImCore,
    client: Option<im_core::ImClient>,
    state: StateRoot,
}

struct ClientInner {
    lifecycle: AtomicU8,
    environment: RwLock<Option<Environment>>,
    mutation: tokio::sync::Mutex<()>,
    cancellation: CancellationToken,
    closed: Notify,
    operation_timeout: Duration,
    sync_timeout: Duration,
}

struct OperationGuard<'a> {
    guard: RwLockReadGuard<'a, Option<Environment>>,
}

impl OperationGuard<'_> {
    fn environment(&self) -> SafeResult<&Environment> {
        self.guard.as_ref().ok_or_else(SafeError::closed)
    }

    fn client(&self) -> SafeResult<&im_core::ImClient> {
        self.environment()?.client.as_ref().ok_or_else(|| {
            SafeError::new(
                "identity_required",
                "A registered IM identity is required.",
                false,
            )
        })
    }
}

impl ClientInner {
    async fn operation(&self) -> SafeResult<OperationGuard<'_>> {
        if self.lifecycle.load(Ordering::Acquire) != LIFECYCLE_OPEN {
            return Err(SafeError::closed());
        }
        let guard = tokio::select! {
            _ = self.cancellation.cancelled() => return Err(SafeError::closed()),
            guard = self.environment.read() => guard,
        };
        if self.lifecycle.load(Ordering::Acquire) != LIFECYCLE_OPEN {
            return Err(SafeError::closed());
        }
        Ok(OperationGuard { guard })
    }

    async fn write_operation(&self) -> SafeResult<RwLockWriteGuard<'_, Option<Environment>>> {
        if self.lifecycle.load(Ordering::Acquire) != LIFECYCLE_OPEN {
            return Err(SafeError::closed());
        }
        let guard = tokio::select! {
            _ = self.cancellation.cancelled() => return Err(SafeError::closed()),
            guard = self.environment.write() => guard,
        };
        if self.lifecycle.load(Ordering::Acquire) != LIFECYCLE_OPEN {
            return Err(SafeError::closed());
        }
        Ok(guard)
    }

    async fn wait_im<T, F>(&self, future: F, timeout: Duration) -> SafeResult<T>
    where
        F: Future<Output = im_core::ImResult<T>>,
    {
        self.wait_safe(
            async move { future.await.map_err(SafeError::from_im) },
            timeout,
        )
        .await
    }

    async fn wait_safe<T, F>(&self, future: F, timeout: Duration) -> SafeResult<T>
    where
        F: Future<Output = SafeResult<T>>,
    {
        tokio::select! {
            _ = self.cancellation.cancelled() => Err(SafeError::cancelled()),
            outcome = tokio::time::timeout(timeout, future) => {
                outcome.map_err(|_| SafeError::timeout())?
            }
        }
    }

    async fn close(&self) -> SafeResult<()> {
        match self.lifecycle.compare_exchange(
            LIFECYCLE_OPEN,
            LIFECYCLE_CLOSING,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {
                self.cancellation.cancel();
                let mut environment = self.environment.write().await;
                environment.take();
                self.lifecycle.store(LIFECYCLE_CLOSED, Ordering::Release);
                self.closed.notify_waiters();
                Ok(())
            }
            Err(LIFECYCLE_CLOSED) => Ok(()),
            Err(LIFECYCLE_CLOSING) => loop {
                let notified = self.closed.notified();
                if self.lifecycle.load(Ordering::Acquire) == LIFECYCLE_CLOSED {
                    return Ok(());
                }
                notified.await;
            },
            Err(_) => Err(SafeError::internal()),
        }
    }

    fn cancel_from_drop(&self) {
        if self
            .lifecycle
            .compare_exchange(
                LIFECYCLE_OPEN,
                LIFECYCLE_CLOSING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            self.cancellation.cancel();
            if let Ok(mut environment) = self.environment.try_write() {
                environment.take();
                self.lifecycle.store(LIFECYCLE_CLOSED, Ordering::Release);
                self.closed.notify_waiters();
            }
        }
    }
}

#[napi(js_name = "NativeImCoreNodeClient")]
pub struct NativeImCoreNodeClient {
    inner: Arc<ClientInner>,
}

#[napi]
impl NativeImCoreNodeClient {
    #[napi(catch_unwind)]
    pub async fn get_default_identity(&self) -> napi::Result<Option<NodeIdentity>> {
        napi_result(self.get_default_identity_inner().await)
    }

    async fn get_default_identity_inner(&self) -> SafeResult<Option<NodeIdentity>> {
        let operation = self.inner.operation().await?;
        let environment = operation.environment()?;
        let identity = self
            .inner
            .wait_im(
                environment.core.identities().default_identity_async(),
                self.inner.operation_timeout,
            )
            .await?;
        let Some(identity) = identity else {
            return Ok(None);
        };
        let display_name = match environment.client.as_ref() {
            Some(client) => self
                .inner
                .wait_im(
                    client.identity().profile_async(),
                    self.inner.operation_timeout,
                )
                .await
                .ok()
                .and_then(|profile| profile.display_name),
            None => identity.display_name.clone(),
        };
        let metadata = environment.state.metadata();
        let identity_id = identity.id.as_str().to_owned();
        let registered_at_ms = self
            .inner
            .wait_safe(
                async move {
                    tokio::task::spawn_blocking(move || metadata.ensure_registered_at(&identity_id))
                        .await
                        .map_err(|_| SafeError::internal())?
                },
                self.inner.operation_timeout,
            )
            .await?;
        Ok(Some(crate::dto::identity(
            &identity,
            display_name,
            registered_at_ms,
        )))
    }

    #[napi(catch_unwind)]
    pub async fn request_registration_otp(
        &self,
        input: NodeRegistrationInput,
    ) -> napi::Result<NodeOtpChallenge> {
        napi_result(self.request_registration_otp_inner(input).await)
    }

    async fn request_registration_otp_inner(
        &self,
        input: NodeRegistrationInput,
    ) -> SafeResult<NodeOtpChallenge> {
        let _mutation = self.inner.mutation.lock().await;
        let operation = self.inner.operation().await?;
        let environment = operation.environment()?;
        ensure_unregistered(&environment.core, &self.inner).await?;
        let request = registration_request(input.handle, input.phone, None)?;
        let challenge = self
            .inner
            .wait_im(
                environment
                    .core
                    .identities()
                    .request_registration_otp_async(request),
                self.inner.operation_timeout,
            )
            .await?;
        Ok(NodeOtpChallenge {
            retry_after_seconds: challenge.retry_after_seconds,
            retry_at: challenge.retry_at,
        })
    }

    #[napi(catch_unwind)]
    pub async fn complete_registration(
        &self,
        input: NodeRegistrationWithOtp,
    ) -> napi::Result<NodeIdentity> {
        napi_result(self.complete_registration_inner(input).await)
    }

    async fn complete_registration_inner(
        &self,
        input: NodeRegistrationWithOtp,
    ) -> SafeResult<NodeIdentity> {
        let otp = input.otp.trim();
        if !(4..=12).contains(&otp.len()) || !otp.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(SafeError::new(
                "invalid_otp",
                "The registration OTP is invalid.",
                false,
            ));
        }
        let _mutation = self.inner.mutation.lock().await;
        let mut operation = self.inner.write_operation().await?;
        let environment = operation.as_mut().ok_or_else(SafeError::closed)?;
        ensure_unregistered(&environment.core, &self.inner).await?;
        let request = registration_request(input.handle, input.phone, Some(otp.to_owned()))?;
        let result = self
            .inner
            .wait_im(
                environment.core.identities().register_handle_async(request),
                self.inner.operation_timeout,
            )
            .await?;
        if result.state == im_core::identity::HandleRegistrationState::JoinRequired {
            return Err(SafeError::new(
                "join_required",
                "The Handle requires an existing-device join flow.",
                false,
            ));
        }
        if result.state != im_core::identity::HandleRegistrationState::Registered {
            return Err(SafeError::internal());
        }
        let identity = result.identity.ok_or_else(SafeError::internal)?;
        let client = self
            .inner
            .wait_im(
                environment
                    .core
                    .client_async(im_core::identity::IdentitySelector::Default),
                self.inner.operation_timeout,
            )
            .await?;
        environment.client = Some(client);
        let metadata = environment.state.metadata();
        let identity_id = identity.id.as_str().to_owned();
        let registered_at_ms =
            tokio::task::spawn_blocking(move || metadata.ensure_registered_at(&identity_id))
                .await
                .map_err(|_| SafeError::internal())??;
        environment.state.harden_permissions()?;
        Ok(crate::dto::identity(
            &identity,
            identity.display_name.clone(),
            registered_at_ms,
        ))
    }

    #[napi(catch_unwind)]
    pub async fn update_display_name(&self, display_name: String) -> napi::Result<NodeIdentity> {
        napi_result(self.update_display_name_inner(display_name).await)
    }

    async fn update_display_name_inner(&self, display_name: String) -> SafeResult<NodeIdentity> {
        let display_name = display_name.trim().to_owned();
        if display_name.is_empty() {
            return Err(SafeError::new(
                "invalid_input",
                "The display name must not be empty.",
                false,
            ));
        }
        let _mutation = self.inner.mutation.lock().await;
        let operation = self.inner.operation().await?;
        let environment = operation.environment()?;
        let client = operation.client()?;
        let profile = self
            .inner
            .wait_im(
                client
                    .identity()
                    .update_profile_async(im_core::identity::ProfilePatch {
                        display_name: Some(display_name),
                        ..Default::default()
                    }),
                self.inner.operation_timeout,
            )
            .await?;
        let metadata = environment.state.metadata();
        let identity = client.current_identity().clone();
        let identity_id = identity.id.as_str().to_owned();
        let registered_at_ms =
            tokio::task::spawn_blocking(move || metadata.ensure_registered_at(&identity_id))
                .await
                .map_err(|_| SafeError::internal())??;
        environment.state.harden_permissions()?;
        Ok(crate::dto::identity(
            &identity,
            profile.display_name,
            registered_at_ms,
        ))
    }

    #[napi(catch_unwind)]
    pub async fn resolve_peer(&self, peer: String) -> napi::Result<NodePeer> {
        napi_result(self.resolve_peer_inner(peer).await)
    }

    async fn resolve_peer_inner(&self, peer: String) -> SafeResult<NodePeer> {
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let peer =
            im_core::ids::PeerRef::parse(peer, client.did_domain()).map_err(SafeError::from_im)?;
        let resolution = self
            .inner
            .wait_im(
                client.directory().resolve_peer_async(peer),
                self.inner.operation_timeout,
            )
            .await?;
        self.inner
            .wait_im(
                client.messages().ensure_conversation_async(
                    im_core::messages::ConversationReadRef::new(resolution.conversation_id.clone())
                        .map_err(SafeError::from_im)?,
                ),
                self.inner.operation_timeout,
            )
            .await?;
        Ok(crate::dto::peer(resolution))
    }

    #[napi(catch_unwind)]
    pub async fn sync_now(&self, input: Option<NodeSyncOptions>) -> napi::Result<NodeSyncResult> {
        napi_result(self.sync_now_inner(input).await)
    }

    async fn sync_now_inner(&self, input: Option<NodeSyncOptions>) -> SafeResult<NodeSyncResult> {
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let (request, timeout) = sync_request(input, self.inner.sync_timeout)?;
        let outcome = self
            .inner
            .wait_im(client.messages().sync_now_async(request), timeout)
            .await?;
        Ok(crate::dto::sync_result(outcome))
    }

    #[napi(catch_unwind)]
    pub async fn list_conversations(
        &self,
        input: Option<NodePageInput>,
    ) -> napi::Result<NodePageOfConversations> {
        napi_result(self.list_conversations_inner(input).await)
    }

    async fn list_conversations_inner(
        &self,
        input: Option<NodePageInput>,
    ) -> SafeResult<NodePageOfConversations> {
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let outcome = self
            .inner
            .wait_im(
                client
                    .messages()
                    .sync_now_async(im_core::messages::MessageSyncRequest {
                        reason: "node_list_conversations".to_owned(),
                        limit: Some(100),
                    }),
                self.inner.sync_timeout,
            )
            .await?;
        ensure_sync_readable(outcome.status)?;
        let input = input.unwrap_or(NodePageInput {
            cursor: None,
            limit: None,
        });
        let page = self
            .inner
            .wait_im(
                client
                    .messages()
                    .conversations_async(im_core::messages::ConversationQuery {
                        limit: page_limit(input.limit)?,
                        cursor: input
                            .cursor
                            .map(im_core::ids::Cursor::parse)
                            .transpose()
                            .map_err(SafeError::from_im)?,
                        include_groups: true,
                        include_direct: true,
                        unread_only: false,
                    }),
                self.inner.operation_timeout,
            )
            .await?;
        crate::dto::conversations(page, client.did().as_str())
    }

    #[napi(catch_unwind)]
    pub async fn get_history(&self, input: NodeHistoryInput) -> napi::Result<NodePageOfMessages> {
        napi_result(self.get_history_inner(input).await)
    }

    async fn get_history_inner(&self, input: NodeHistoryInput) -> SafeResult<NodePageOfMessages> {
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let conversation = im_core::messages::ConversationReadRef::new(&input.conversation_id)
            .map_err(SafeError::from_im)?;
        let page = self
            .inner
            .wait_im(
                client.messages().conversation_history_async(
                    conversation,
                    im_core::messages::HistoryQuery {
                        limit: page_limit(input.limit)?,
                        cursor: input
                            .cursor
                            .map(im_core::ids::Cursor::parse)
                            .transpose()
                            .map_err(SafeError::from_im)?,
                        inbox_history_options: None,
                    },
                ),
                self.inner.operation_timeout,
            )
            .await?;
        crate::dto::messages(page, &input.conversation_id)
    }

    #[napi(catch_unwind)]
    pub async fn mark_conversation_read(
        &self,
        conversation_id: String,
    ) -> napi::Result<NodeMarkReadResult> {
        napi_result(self.mark_conversation_read_inner(conversation_id).await)
    }

    async fn mark_conversation_read_inner(
        &self,
        conversation_id: String,
    ) -> SafeResult<NodeMarkReadResult> {
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let result = self
            .inner
            .wait_im(
                client.messages().mark_conversation_read_async(
                    im_core::messages::MarkConversationReadRequest {
                        conversation: im_core::messages::ConversationReadRef::new(conversation_id)
                            .map_err(SafeError::from_im)?,
                        watermark: None,
                        fallback_max_message_ids: Some(500),
                    },
                ),
                self.inner.operation_timeout,
            )
            .await?;
        Ok(NodeMarkReadResult {
            updated_count: result.updated_count,
            remote_acknowledged: result.remote_acknowledged,
            partial: result.partial,
            fallback_used: result.fallback_used,
            pending_remote_ack: result.pending_remote_ack,
            warnings: result.warnings,
        })
    }

    #[napi(catch_unwind)]
    pub async fn send_text(&self, input: NodeSendTextInput) -> napi::Result<NodeMessage> {
        napi_result(self.send_text_inner(input).await)
    }

    async fn send_text_inner(&self, input: NodeSendTextInput) -> SafeResult<NodeMessage> {
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let conversation_id = input.conversation_id;
        let result = self
            .inner
            .wait_im(
                client.messages().send_conversation_text_async(
                    im_core::messages::SendConversationTextRequest {
                        conversation: im_core::messages::ConversationReadRef::new(&conversation_id)
                            .map_err(SafeError::from_im)?,
                        text: input.text,
                        markdown: input.markdown.unwrap_or(false),
                        security: im_core::messages::MessageSecurityMode::DefaultPlain,
                        client_message_id: optional_message_id(input.client_message_id)?,
                        idempotency_key: non_empty_optional(input.idempotency_key),
                        wait_for_final_acceptance: false,
                        delegated_signing: None,
                    },
                ),
                self.inner.operation_timeout,
            )
            .await?;
        crate::dto::sent_message(result.message, &conversation_id)
    }

    #[napi(catch_unwind)]
    pub async fn send_attachment(
        &self,
        input: NodeSendAttachmentInput,
    ) -> napi::Result<NodeMessage> {
        napi_result(self.send_attachment_inner(input).await)
    }

    async fn send_attachment_inner(
        &self,
        input: NodeSendAttachmentInput,
    ) -> SafeResult<NodeMessage> {
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let conversation_id = input.conversation_id;
        let result = self
            .inner
            .wait_im(
                client.attachments().send_conversation_async(
                    im_core::attachments::SendConversationAttachmentRequest {
                        conversation: im_core::messages::ConversationReadRef::new(&conversation_id)
                            .map_err(SafeError::from_im)?,
                        input: im_core::attachments::AttachmentInput::Bytes {
                            filename: Some(input.file_name.clone()),
                            mime_type: Some(input.mime_type.clone()),
                            bytes: input.bytes.to_vec(),
                        },
                        caption: non_empty_optional(input.caption),
                        mention_payload: None,
                        mime_type: Some(input.mime_type),
                        filename: Some(input.file_name),
                        security: im_core::messages::MessageSecurityMode::DefaultPlain,
                        client_message_id: optional_message_id(input.client_message_id)?,
                        idempotency_key: non_empty_optional(input.idempotency_key),
                        wait_for_final_acceptance: false,
                    },
                ),
                self.inner.operation_timeout,
            )
            .await?;
        crate::dto::uploaded_attachment(result, &conversation_id)
    }

    #[napi(catch_unwind)]
    pub async fn download_attachment(
        &self,
        input: NodeDownloadAttachmentInput,
    ) -> napi::Result<NodeDownload> {
        napi_result(self.download_attachment_inner(input).await)
    }

    async fn download_attachment_inner(
        &self,
        input: NodeDownloadAttachmentInput,
    ) -> SafeResult<NodeDownload> {
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let timeout = optional_timeout(input.timeout_ms, self.inner.operation_timeout)?;
        let result = self
            .inner
            .wait_im(
                client.attachments().download_conversation_async(
                    im_core::attachments::DownloadConversationAttachmentRequest {
                        conversation: im_core::messages::ConversationReadRef::new(
                            input.conversation_id,
                        )
                        .map_err(SafeError::from_im)?,
                        message_id: im_core::ids::MessageId::parse(input.message_id)
                            .map_err(SafeError::from_im)?,
                        attachment_id: non_empty_optional(input.attachment_id),
                        destination: im_core::attachments::AttachmentDestination::Memory,
                        overwrite: false,
                    },
                ),
                timeout,
            )
            .await?;
        crate::dto::downloaded_attachment(result)
    }

    #[napi(catch_unwind)]
    pub async fn close(&self) -> napi::Result<()> {
        napi_result(self.inner.close().await)
    }
}

impl Drop for NativeImCoreNodeClient {
    fn drop(&mut self) {
        self.inner.cancel_from_drop();
    }
}

pub(crate) async fn open(options: NodeOpenOptions) -> SafeResult<NativeImCoreNodeClient> {
    let operation_timeout = optional_timeout(
        options.operation_timeout_ms,
        Duration::from_millis(u64::from(DEFAULT_OPERATION_TIMEOUT_MS)),
    )?;
    let sync_timeout = optional_timeout(
        options.sync_timeout_ms,
        Duration::from_millis(u64::from(DEFAULT_SYNC_TIMEOUT_MS)),
    )?;
    let state_root = options.state_root.clone();
    let state = tokio::task::spawn_blocking(move || StateRoot::open(PathBuf::from(state_root)))
        .await
        .map_err(|_| SafeError::internal())??;
    let paths = state.paths();
    let config = core_config(options)?;
    let core = im_core::ImCore::open_with_options(
        config,
        paths,
        im_core::ImCoreOpenOptions::file_compat(),
    )
    .await
    .map_err(SafeError::from_im)?;
    core.bootstrap()
        .initialize_local_state_async()
        .await
        .map_err(SafeError::from_im)?;
    state.harden_permissions()?;
    let client = match core
        .identities()
        .default_identity_async()
        .await
        .map_err(SafeError::from_im)?
    {
        Some(_) => Some(
            core.client_async(im_core::identity::IdentitySelector::Default)
                .await
                .map_err(SafeError::from_im)?,
        ),
        None => None,
    };
    Ok(NativeImCoreNodeClient {
        inner: Arc::new(ClientInner {
            lifecycle: AtomicU8::new(LIFECYCLE_OPEN),
            environment: RwLock::new(Some(Environment {
                core,
                client,
                state,
            })),
            mutation: tokio::sync::Mutex::new(()),
            cancellation: CancellationToken::new(),
            closed: Notify::new(),
            operation_timeout,
            sync_timeout,
        }),
    })
}

fn core_config(options: NodeOpenOptions) -> SafeResult<im_core::ImCoreConfig> {
    let mut config = im_core::ImCoreConfig::new(
        im_core::ServiceEndpoint::parse(options.service_base_url).map_err(SafeError::from_im)?,
        options.did_domain,
    )
    .map_err(SafeError::from_im)?;
    config.user_service_endpoint = optional_endpoint(options.user_service_endpoint)?;
    config.message_service_endpoint = optional_endpoint(options.message_service_endpoint)?;
    config.anp_service_endpoint = optional_endpoint(options.anp_service_endpoint)?;
    config.anp_service_did = options
        .anp_service_did
        .map(im_core::ids::Did::parse)
        .transpose()
        .map_err(SafeError::from_im)?;
    config.transport_policy = im_core::MessageTransportPolicy::HttpOnly;
    Ok(config)
}

fn optional_endpoint(value: Option<String>) -> SafeResult<Option<im_core::ServiceEndpoint>> {
    value
        .map(im_core::ServiceEndpoint::parse)
        .transpose()
        .map_err(SafeError::from_im)
}

fn registration_request(
    handle: String,
    phone: String,
    otp: Option<String>,
) -> SafeResult<im_core::identity::RegisterHandleRequest> {
    let requested_handle = im_core::ids::Handle::parse(handle, "").map_err(SafeError::from_im)?;
    Ok(im_core::identity::RegisterHandleRequest {
        local_alias: Some("default".to_owned()),
        requested_handle,
        verification: im_core::identity::VerificationInput::Phone { phone, otp },
        invite_code: None,
        profile: im_core::identity::InitialProfile {
            display_name: None,
            avatar_url: None,
        },
        make_default: true,
    })
}

async fn ensure_unregistered(core: &im_core::ImCore, inner: &ClientInner) -> SafeResult<()> {
    if inner
        .wait_im(
            core.identities().default_identity_async(),
            inner.operation_timeout,
        )
        .await?
        .is_some()
    {
        return Err(SafeError::new(
            "already_registered",
            "An IM identity is already registered.",
            false,
        ));
    }
    Ok(())
}

fn page_limit(value: Option<u32>) -> SafeResult<im_core::ids::PageLimit> {
    im_core::ids::PageLimit::new(value.unwrap_or(50)).map_err(SafeError::from_im)
}

fn optional_message_id(value: Option<String>) -> SafeResult<Option<im_core::ids::MessageId>> {
    value
        .map(im_core::ids::MessageId::parse)
        .transpose()
        .map_err(SafeError::from_im)
}

fn non_empty_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn optional_timeout(value: Option<u32>, default: Duration) -> SafeResult<Duration> {
    match value {
        Some(0) => Err(SafeError::new(
            "invalid_input",
            "The timeout must be greater than zero.",
            false,
        )),
        Some(value) if value > MAX_TIMEOUT_MS => Err(SafeError::new(
            "invalid_input",
            "The timeout exceeds the supported maximum.",
            false,
        )),
        Some(value) => Ok(Duration::from_millis(u64::from(value))),
        None => Ok(default),
    }
}

fn sync_request(
    input: Option<NodeSyncOptions>,
    default_timeout: Duration,
) -> SafeResult<(im_core::messages::MessageSyncRequest, Duration)> {
    let input = input.unwrap_or(NodeSyncOptions {
        reason: None,
        limit: None,
        timeout_ms: None,
    });
    Ok((
        im_core::messages::MessageSyncRequest {
            reason: input
                .reason
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "node_explicit".to_owned()),
            limit: input.limit,
        },
        optional_timeout(input.timeout_ms, default_timeout)?,
    ))
}

fn ensure_sync_readable(status: im_core::messages::MessageSyncStatus) -> SafeResult<()> {
    match status {
        im_core::messages::MessageSyncStatus::Idle
        | im_core::messages::MessageSyncStatus::Changed => Ok(()),
        other => Err(SafeError::sync_outcome(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(state_root: &std::path::Path) -> NodeOpenOptions {
        NodeOpenOptions {
            state_root: state_root.display().to_string(),
            service_base_url: "https://example.test".to_owned(),
            did_domain: "example.test".to_owned(),
            user_service_endpoint: None,
            message_service_endpoint: None,
            anp_service_endpoint: None,
            anp_service_did: None,
            operation_timeout_ms: Some(1_000),
            sync_timeout_ms: Some(100),
        }
    }

    #[tokio::test]
    async fn close_is_idempotent_rejects_new_work_and_releases_the_state_lock() {
        let directory = tempfile::tempdir().unwrap();
        let client = open(options(directory.path())).await.unwrap();
        client.inner.close().await.unwrap();
        client.inner.close().await.unwrap();
        assert_eq!(
            client.get_default_identity_inner().await.unwrap_err().code,
            "client_closed"
        );
        open(options(directory.path())).await.unwrap();
    }

    #[tokio::test]
    async fn close_cancels_an_inflight_operation_before_releasing_state() {
        let directory = tempfile::tempdir().unwrap();
        let client = open(options(directory.path())).await.unwrap();
        let operation = client.inner.operation().await.unwrap();
        let inner = client.inner.clone();
        let close = tokio::spawn(async move { inner.close().await });
        tokio::task::yield_now().await;
        assert!(!close.is_finished());
        drop(operation);
        close.await.unwrap().unwrap();
        open(options(directory.path())).await.unwrap();
    }

    #[test]
    fn timeouts_are_bounded() {
        assert!(optional_timeout(Some(0), Duration::from_secs(1)).is_err());
        assert!(optional_timeout(Some(MAX_TIMEOUT_MS + 1), Duration::from_secs(1)).is_err());
        assert_eq!(
            optional_timeout(Some(10), Duration::from_secs(1)).unwrap(),
            Duration::from_millis(10)
        );
    }
}
