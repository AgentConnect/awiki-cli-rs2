use std::collections::HashSet;
use std::collections::VecDeque;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
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
const LIST_CONVERSATIONS_SYNC_REASON: &str = "foreground_reconcile";
const DEFAULT_REALTIME_EVENT_BUFFER: u32 = 128;
const MAX_REALTIME_EVENT_BUFFER: u32 = 4_096;
const DEFAULT_RECONNECT_BASE_DELAY_MS: u32 = 1_000;
const DEFAULT_RECONNECT_MAX_DELAY_MS: u32 = 30_000;
const DEFAULT_MAIL_INBOX_LIMIT: u32 = 20;
const MAX_MAIL_INBOX_LIMIT: u32 = 100;
const MAX_MAIL_MESSAGE_IDS: usize = 100;
const MAX_MAIL_RECIPIENTS: usize = 20;

type BoxImFuture<'a, T> = Pin<Box<dyn Future<Output = im_core::ImResult<T>> + Send + 'a>>;

// Keep deep Core futures behind dynamic dispatch before N-API adds timeout/select wrappers.
fn box_im_future<'a, T, F>(future: F) -> BoxImFuture<'a, T>
where
    F: Future<Output = im_core::ImResult<T>> + Send + 'a,
{
    Box::pin(future)
}

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
    options: NodeOpenOptions,
    identity_provider: std::sync::Mutex<Option<crate::external_identity::ExternalIdentityCustody>>,
    realtime: tokio::sync::Mutex<Option<RealtimeSlot>>,
    next_realtime_id: AtomicU64,
    #[cfg(test)]
    realtime_close_error: std::sync::Mutex<Option<SafeError>>,
}

struct RealtimeSlot {
    id: u64,
    session: im_core::realtime::RealtimeSession,
}

struct RealtimeEventReader {
    events: im_core::realtime::RealtimeEventStream,
    pending: VecDeque<NodeRealtimeEvent>,
    connected_once: bool,
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
                let realtime_result = self.teardown_realtime_for_close().await;
                self.complete_close(realtime_result).await
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
            if let Ok(mut realtime) = self.realtime.try_lock() {
                realtime.take();
            }
            if let Ok(mut environment) = self.environment.try_write() {
                environment.take();
                self.identity_provider
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                self.lifecycle.store(LIFECYCLE_CLOSED, Ordering::Release);
                self.closed.notify_waiters();
            }
        }
    }

    async fn complete_close(&self, realtime_result: SafeResult<()>) -> SafeResult<()> {
        let mut environment = self.environment.write().await;
        environment.take();
        self.identity_provider
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        self.lifecycle.store(LIFECYCLE_CLOSED, Ordering::Release);
        self.closed.notify_waiters();
        realtime_result
    }

    async fn teardown_realtime_for_close(&self) -> SafeResult<()> {
        let result = self.stop_realtime(None).await;
        #[cfg(test)]
        if let Some(error) = self
            .realtime_close_error
            .lock()
            .expect("realtime close error lock")
            .take()
        {
            return Err(error);
        }
        result
    }

    async fn stop_realtime(&self, expected_id: Option<u64>) -> SafeResult<()> {
        let slot = {
            let mut realtime = self.realtime.lock().await;
            if expected_id.is_some_and(|id| realtime.as_ref().is_some_and(|slot| slot.id != id)) {
                return Ok(());
            }
            realtime.take()
        };
        let Some(mut slot) = slot else {
            return Ok(());
        };
        slot.session.stop().await.map_err(SafeError::from_im)?;
        slot.session.join().await.map_err(SafeError::from_im)?;
        Ok(())
    }
}

#[napi(js_name = "NativeRealtimeSession")]
pub struct NativeRealtimeSession {
    id: u64,
    client: Arc<ClientInner>,
    events: Arc<tokio::sync::Mutex<RealtimeEventReader>>,
    status: tokio::sync::watch::Receiver<im_core::realtime::RealtimeStatus>,
}

#[napi]
impl NativeRealtimeSession {
    #[napi(catch_unwind)]
    pub async fn next_event(&self) -> napi::Result<Option<NodeRealtimeEvent>> {
        napi_result(self.next_event_inner().await)
    }

    async fn next_event_inner(&self) -> SafeResult<Option<NodeRealtimeEvent>> {
        let mut reader = self.events.lock().await;
        if let Some(event) = reader.pending.pop_front() {
            return Ok(Some(event));
        }
        let event = tokio::select! {
            _ = self.client.cancellation.cancelled() => return Ok(None),
            event = reader.events.recv() => event,
        };
        let Some(event) = event else {
            return Ok(None);
        };
        let connected_once = reader.connected_once;
        let mapped = map_realtime_event(event, connected_once);
        if mapped.iter().any(|event| {
            event.kind == "connection_state_changed" && event.state.as_deref() == Some("connected")
        }) {
            reader.connected_once = true;
        }
        reader.pending.extend(mapped);
        Ok(reader.pending.pop_front())
    }

    #[napi(catch_unwind)]
    pub async fn get_status(&self) -> napi::Result<NodeRealtimeStatus> {
        let status = self.status.borrow().clone();
        Ok(node_realtime_status(status))
    }

    #[napi(catch_unwind)]
    pub async fn stop(&self) -> napi::Result<()> {
        napi_result(self.client.stop_realtime(Some(self.id)).await)
    }
}

#[napi(js_name = "NativeImCoreNodeClient")]
pub struct NativeImCoreNodeClient {
    inner: Arc<ClientInner>,
}

/// Opaque, single-use external HTTP authentication attempt.
#[napi(js_name = "NativeExternalHttpAuthAttempt")]
pub struct NativeExternalHttpAuthAttempt {
    inner: Arc<ClientInner>,
    attempt: tokio::sync::Mutex<Option<im_core::ExternalHttpAuthAttempt>>,
    target_url: String,
    method: String,
    header_patch: Vec<(String, String)>,
    retry_count: u32,
}

impl NativeExternalHttpAuthAttempt {
    fn new(inner: Arc<ClientInner>, attempt: im_core::ExternalHttpAuthAttempt) -> Self {
        let target_url = attempt.target_url().to_owned();
        let method = attempt.method().to_owned();
        let header_patch = attempt
            .header_patch()
            .iter()
            .map(|header| (header.name().to_owned(), header.value().to_owned()))
            .collect();
        let retry_count = u32::from(attempt.retry_count());
        Self {
            inner,
            attempt: tokio::sync::Mutex::new(Some(attempt)),
            target_url,
            method,
            header_patch,
            retry_count,
        }
    }

    async fn handle_response_inner(
        &self,
        input: NodeExternalHttpResponse,
    ) -> SafeResult<Option<Self>> {
        let attempt = self.attempt.lock().await.take().ok_or_else(|| {
            SafeError::new(
                "external_http_attempt_consumed",
                "The external HTTP authentication attempt was already completed.",
                false,
            )
        })?;
        let response = crate::dto::external_http_response(input)?;
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let decision = self
            .inner
            .wait_im(
                client
                    .external_http_auth()
                    .handle_response_async(attempt, response),
                self.inner.operation_timeout,
            )
            .await?;
        match decision {
            im_core::ExternalHttpAuthDecision::Complete => Ok(None),
            im_core::ExternalHttpAuthDecision::Retry(attempt) => {
                Ok(Some(Self::new(self.inner.clone(), attempt)))
            }
        }
    }
}

impl std::fmt::Debug for NativeExternalHttpAuthAttempt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeExternalHttpAuthAttempt")
            .field("target_url", &"<redacted-url>")
            .field("method", &self.method)
            .field(
                "header_names",
                &self
                    .header_patch
                    .iter()
                    .map(|(name, _)| name)
                    .collect::<Vec<_>>(),
            )
            .field("retry_count", &self.retry_count)
            .finish()
    }
}

#[napi]
impl NativeExternalHttpAuthAttempt {
    #[napi(catch_unwind)]
    pub fn get_target_url(&self) -> String {
        self.target_url.clone()
    }

    #[napi(catch_unwind)]
    pub fn get_method(&self) -> String {
        self.method.clone()
    }

    #[napi(catch_unwind)]
    pub fn get_header_patch(&self) -> Vec<NodeExternalHttpHeader> {
        self.header_patch
            .iter()
            .map(|(name, value)| NodeExternalHttpHeader {
                name: name.clone(),
                value: value.clone(),
            })
            .collect()
    }

    #[napi(catch_unwind)]
    pub fn get_retry_count(&self) -> u32 {
        self.retry_count
    }

    #[napi(catch_unwind)]
    pub async fn handle_response(
        &self,
        input: NodeExternalHttpResponse,
    ) -> napi::Result<Option<NativeExternalHttpAuthAttempt>> {
        napi_result(self.handle_response_inner(input).await)
    }
}

#[napi]
impl NativeImCoreNodeClient {
    #[napi(catch_unwind)]
    pub async fn prepare_external_http_request(
        &self,
        input: NodeExternalHttpRequest,
    ) -> napi::Result<NativeExternalHttpAuthAttempt> {
        napi_result(self.prepare_external_http_request_inner(input).await)
    }

    async fn prepare_external_http_request_inner(
        &self,
        input: NodeExternalHttpRequest,
    ) -> SafeResult<NativeExternalHttpAuthAttempt> {
        let request = crate::dto::external_http_request(input)?;
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let attempt = self
            .inner
            .wait_im(
                client.external_http_auth().prepare_async(request),
                self.inner.operation_timeout,
            )
            .await?;
        Ok(NativeExternalHttpAuthAttempt::new(
            self.inner.clone(),
            attempt,
        ))
    }

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
                environment.core.identities().register_handle_async(request),
                self.inner.operation_timeout,
            )
            .await?;
        if challenge.state != im_core::identity::HandleRegistrationState::OtpSent {
            return Err(SafeError::internal());
        }
        Ok(NodeOtpChallenge {
            retry_after_seconds: challenge
                .retry_after_seconds
                .ok_or_else(SafeError::internal)?,
            retry_at: challenge.retry_at.ok_or_else(SafeError::internal)?,
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
        let outcome = self.complete_registration_with_outcome_inner(input).await?;
        outcome.identity.ok_or_else(|| {
            SafeError::new(
                "join_required",
                "The Handle requires an existing-identity flow.",
                false,
            )
        })
    }

    #[napi(catch_unwind)]
    pub async fn complete_registration_with_outcome(
        &self,
        input: NodeRegistrationWithOtp,
    ) -> napi::Result<NodeRegistrationOutcome> {
        napi_result(self.complete_registration_with_outcome_inner(input).await)
    }

    async fn complete_registration_with_outcome_inner(
        &self,
        input: NodeRegistrationWithOtp,
    ) -> SafeResult<NodeRegistrationOutcome> {
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
            let preparation = result.join_required.ok_or_else(SafeError::internal)?;
            return Ok(crate::dto::existing_handle_registration_outcome(
                preparation,
                result.warnings,
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
        Ok(NodeRegistrationOutcome {
            status: "registered".to_owned(),
            identity: Some(crate::dto::identity(
                &identity,
                identity.display_name.clone(),
                registered_at_ms,
            )),
            existing_handle: None,
            warnings: result.warnings,
        })
    }

    #[napi(catch_unwind)]
    pub async fn begin_prepared_registration_join(
        &self,
        input: NodePreparedRegistrationJoinInput,
    ) -> napi::Result<NodePreparedRegistrationJoinProgress> {
        napi_result(self.begin_prepared_registration_join_inner(input).await)
    }

    async fn begin_prepared_registration_join_inner(
        &self,
        input: NodePreparedRegistrationJoinInput,
    ) -> SafeResult<NodePreparedRegistrationJoinProgress> {
        let _mutation = self.inner.mutation.lock().await;
        let mut operation = self.inner.write_operation().await?;
        let environment = operation.as_mut().ok_or_else(SafeError::closed)?;
        ensure_unregistered(&environment.core, &self.inner).await?;
        let value = self
            .inner
            .wait_im(
                environment
                    .core
                    .handle_recovery()
                    .begin_prepared_registration_device_join(
                        im_core::identity::BeginPreparedRegistrationDeviceJoinRequest {
                            preparation_id: input.continuation_id,
                            operation_id: input.operation_id,
                            ttl_seconds: u64::from(input.ttl_seconds.unwrap_or(600)),
                            user_presence_confirmed: input.user_presence_confirmed,
                        },
                    ),
                self.inner.operation_timeout,
            )
            .await?;
        let identity = self
            .refresh_prepared_registration_client(environment, &value)
            .await?;
        environment.state.harden_permissions()?;
        Ok(crate::dto::prepared_registration_join_progress(
            value, identity,
        ))
    }

    #[napi(catch_unwind)]
    pub async fn resume_prepared_registration_join(
        &self,
        input: NodePreparedRegistrationJoinResumeInput,
    ) -> napi::Result<NodePreparedRegistrationJoinProgress> {
        napi_result(self.resume_prepared_registration_join_inner(input).await)
    }

    async fn resume_prepared_registration_join_inner(
        &self,
        input: NodePreparedRegistrationJoinResumeInput,
    ) -> SafeResult<NodePreparedRegistrationJoinProgress> {
        let _mutation = self.inner.mutation.lock().await;
        let mut operation = self.inner.write_operation().await?;
        let environment = operation.as_mut().ok_or_else(SafeError::closed)?;
        if let Some(session) = local_new_device_session(environment, &input.join_session_id)? {
            if matches!(
                session.phase,
                im_core::identity::DeviceJoinLocalPhase::Cancelled
                    | im_core::identity::DeviceJoinLocalPhase::Expired
            ) {
                return crate::dto::terminal_prepared_registration_join_progress(session);
            }
        }
        let value = self
            .inner
            .wait_im(
                environment
                    .core
                    .handle_recovery()
                    .resume_authorized_join_activation(&input.join_session_id),
                self.inner.operation_timeout,
            )
            .await?;
        let identity = self
            .refresh_prepared_registration_client(environment, &value)
            .await?;
        environment.state.harden_permissions()?;
        Ok(crate::dto::prepared_registration_join_progress(
            value, identity,
        ))
    }

    #[napi(catch_unwind)]
    pub async fn list_local_device_join_sessions(
        &self,
    ) -> napi::Result<Vec<NodeLocalDeviceJoinSession>> {
        napi_result(self.list_local_device_join_sessions_inner().await)
    }

    async fn list_local_device_join_sessions_inner(
        &self,
    ) -> SafeResult<Vec<NodeLocalDeviceJoinSession>> {
        let operation = self.inner.operation().await?;
        operation
            .environment()?
            .core
            .device_join()
            .local_sessions()
            .map(|sessions| {
                sessions
                    .into_iter()
                    .map(crate::dto::local_device_join_session)
                    .collect()
            })
            .map_err(SafeError::from_im)
    }

    #[napi(catch_unwind)]
    pub async fn cancel_prepared_registration_join(
        &self,
        input: NodePreparedRegistrationJoinCancelInput,
    ) -> napi::Result<NodeLocalDeviceJoinSession> {
        napi_result(self.cancel_prepared_registration_join_inner(input).await)
    }

    async fn cancel_prepared_registration_join_inner(
        &self,
        input: NodePreparedRegistrationJoinCancelInput,
    ) -> SafeResult<NodeLocalDeviceJoinSession> {
        let _mutation = self.inner.mutation.lock().await;
        let operation = self.inner.operation().await?;
        let environment = operation.environment()?;
        let session = self
            .inner
            .wait_im(
                environment
                    .core
                    .device_join()
                    .cancel_new_device_join(&input.join_session_id),
                self.inner.operation_timeout,
            )
            .await?;
        environment.state.harden_permissions()?;
        Ok(crate::dto::local_device_join_session(session))
    }

    async fn refresh_prepared_registration_client(
        &self,
        environment: &mut Environment,
        progress: &im_core::identity::AuthorizedJoinActivationProgress,
    ) -> SafeResult<Option<NodeIdentity>> {
        if progress.join.session.phase != im_core::identity::DeviceJoinLocalPhase::Authorized
            || progress.join.remote_state != im_core::identity::DeviceJoinRemoteState::Consumed
        {
            return Ok(None);
        }
        let selector = im_core::identity::IdentitySelector::Did(progress.join.session.did.clone());
        let identity = self
            .inner
            .wait_im(
                environment
                    .core
                    .identities()
                    .resolve_async(selector.clone()),
                self.inner.operation_timeout,
            )
            .await?;
        let client = self
            .inner
            .wait_im(
                environment.core.client_async(selector),
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
        Ok(Some(crate::dto::identity(
            &identity,
            identity.display_name.clone(),
            registered_at_ms,
        )))
    }

    #[napi(catch_unwind)]
    pub async fn get_current_device_summary(&self) -> napi::Result<NodeCurrentDeviceSummary> {
        napi_result(self.get_current_device_summary_inner().await)
    }

    async fn get_current_device_summary_inner(&self) -> SafeResult<NodeCurrentDeviceSummary> {
        let operation = self.inner.operation().await?;
        let environment = operation.environment()?;
        operation.client()?;
        self.inner
            .wait_im(
                environment
                    .core
                    .identities()
                    .device_summary_async(im_core::identity::IdentitySelector::Default),
                self.inner.operation_timeout,
            )
            .await
            .map(crate::dto::current_device_summary)
    }

    #[napi(catch_unwind)]
    pub async fn get_device_registry(&self) -> napi::Result<NodeDeviceRegistrySnapshot> {
        napi_result(self.get_device_registry_inner().await)
    }

    async fn get_device_registry_inner(&self) -> SafeResult<NodeDeviceRegistrySnapshot> {
        let _mutation = self.inner.mutation.lock().await;
        let operation = self.inner.operation().await?;
        let environment = operation.environment()?;
        operation.client()?;
        self.inner
            .wait_im(
                environment
                    .core
                    .device_join()
                    .registry(im_core::identity::IdentitySelector::Default),
                self.inner.operation_timeout,
            )
            .await
            .map(crate::dto::device_registry_snapshot)
    }

    #[napi(catch_unwind)]
    pub async fn list_local_device_join_requests(
        &self,
    ) -> napi::Result<Vec<NodeDeviceJoinRequestNotice>> {
        napi_result(self.list_local_device_join_requests_inner().await)
    }

    async fn list_local_device_join_requests_inner(
        &self,
    ) -> SafeResult<Vec<NodeDeviceJoinRequestNotice>> {
        let _mutation = self.inner.mutation.lock().await;
        let operation = self.inner.operation().await?;
        let environment = operation.environment()?;
        operation.client()?;
        self.inner
            .wait_im(
                environment
                    .core
                    .device_join()
                    .local_device_join_requests(im_core::identity::IdentitySelector::Default),
                self.inner.operation_timeout,
            )
            .await
            .map(|items| {
                items
                    .into_iter()
                    .map(crate::dto::device_join_request_notice)
                    .collect()
            })
    }

    #[napi(catch_unwind)]
    pub async fn start_device_join_verification(
        &self,
        input: NodeStartDeviceJoinVerificationInput,
    ) -> napi::Result<NodeAdminDeviceJoinProgress> {
        napi_result(self.start_device_join_verification_inner(input).await)
    }

    async fn start_device_join_verification_inner(
        &self,
        input: NodeStartDeviceJoinVerificationInput,
    ) -> SafeResult<NodeAdminDeviceJoinProgress> {
        let _mutation = self.inner.mutation.lock().await;
        let operation = self.inner.operation().await?;
        let environment = operation.environment()?;
        operation.client()?;
        self.inner
            .wait_im(
                environment
                    .core
                    .device_join()
                    .start_device_join_verification(
                        im_core::identity::IdentitySelector::Default,
                        &input.join_session_id,
                        &input.operation_id,
                        u64::from(input.challenge_ttl_seconds),
                    ),
                self.inner.operation_timeout,
            )
            .await
            .map(crate::dto::admin_device_join_progress)
    }

    #[napi(catch_unwind)]
    pub async fn get_local_device_join_verification_progress(
        &self,
        input: NodeDeviceJoinSessionInput,
    ) -> napi::Result<NodeAdminDeviceJoinProgress> {
        napi_result(
            self.get_local_device_join_verification_progress_inner(input)
                .await,
        )
    }

    async fn get_local_device_join_verification_progress_inner(
        &self,
        input: NodeDeviceJoinSessionInput,
    ) -> SafeResult<NodeAdminDeviceJoinProgress> {
        let operation = self.inner.operation().await?;
        let environment = operation.environment()?;
        operation.client()?;
        self.inner
            .wait_im(
                environment
                    .core
                    .device_join()
                    .local_device_join_verification_progress_async(
                        im_core::identity::IdentitySelector::Default,
                        &input.join_session_id,
                    ),
                self.inner.operation_timeout,
            )
            .await
            .map(crate::dto::admin_device_join_progress)
    }

    #[napi(catch_unwind)]
    pub async fn prepare_device_join_approval(
        &self,
        input: NodePrepareDeviceJoinApprovalInput,
    ) -> napi::Result<NodeDeviceJoinApprovalPrompt> {
        napi_result(self.prepare_device_join_approval_inner(input).await)
    }

    async fn prepare_device_join_approval_inner(
        &self,
        input: NodePrepareDeviceJoinApprovalInput,
    ) -> SafeResult<NodeDeviceJoinApprovalPrompt> {
        let _mutation = self.inner.mutation.lock().await;
        let operation = self.inner.operation().await?;
        let environment = operation.environment()?;
        operation.client()?;
        environment
            .core
            .device_join()
            .prepare_device_join_approval(
                im_core::identity::IdentitySelector::Default,
                &input.join_session_id,
                input.sas_confirmed,
            )
            .map(crate::dto::device_join_approval_prompt)
            .map_err(SafeError::from_im)
    }

    #[napi(catch_unwind)]
    pub async fn confirm_device_join_approval(
        &self,
        input: NodeConfirmDeviceJoinApprovalInput,
    ) -> napi::Result<NodeAdminDeviceJoinProgress> {
        napi_result(self.confirm_device_join_approval_inner(input).await)
    }

    async fn confirm_device_join_approval_inner(
        &self,
        input: NodeConfirmDeviceJoinApprovalInput,
    ) -> SafeResult<NodeAdminDeviceJoinProgress> {
        let _mutation = self.inner.mutation.lock().await;
        let operation = self.inner.operation().await?;
        let environment = operation.environment()?;
        operation.client()?;
        self.inner
            .wait_im(
                environment.core.device_join().confirm_device_join_approval(
                    im_core::identity::DeviceJoinConfirmApprovalRequest {
                        approval_handle: input.approval_handle,
                        user_presence_confirmed: input.user_presence_confirmed,
                    },
                ),
                self.inner.operation_timeout,
            )
            .await
            .map(crate::dto::admin_device_join_progress)
    }

    #[napi(catch_unwind)]
    pub async fn reject_device_join(
        &self,
        input: NodeRejectDeviceJoinInput,
    ) -> napi::Result<NodeAdminDeviceJoinProgress> {
        napi_result(self.reject_device_join_inner(input).await)
    }

    async fn reject_device_join_inner(
        &self,
        input: NodeRejectDeviceJoinInput,
    ) -> SafeResult<NodeAdminDeviceJoinProgress> {
        let reason = match input.reason.as_str() {
            "user_rejected" => im_core::identity::DeviceJoinRejectReason::UserRejected,
            "sas_mismatch" => im_core::identity::DeviceJoinRejectReason::SasMismatch,
            _ => {
                return Err(invalid_input(
                    "The device Join rejection reason is invalid.",
                ))
            }
        };
        let _mutation = self.inner.mutation.lock().await;
        let operation = self.inner.operation().await?;
        let environment = operation.environment()?;
        operation.client()?;
        self.inner
            .wait_im(
                environment.core.device_join().reject_device_join(
                    im_core::identity::IdentitySelector::Default,
                    &input.join_session_id,
                    reason,
                ),
                self.inner.operation_timeout,
            )
            .await
            .map(crate::dto::admin_device_join_progress)
    }

    #[napi(catch_unwind)]
    pub async fn revoke_device(
        &self,
        input: NodeRevokeDeviceInput,
    ) -> napi::Result<NodeDeviceRevokeResult> {
        napi_result(self.revoke_device_inner(input).await)
    }

    async fn revoke_device_inner(
        &self,
        input: NodeRevokeDeviceInput,
    ) -> SafeResult<NodeDeviceRevokeResult> {
        let target_device_id = im_core::ids::ProtocolDeviceId::parse(input.target_device_id)
            .map_err(SafeError::from_im)?;
        let _mutation = self.inner.mutation.lock().await;
        let operation = self.inner.operation().await?;
        let environment = operation.environment()?;
        operation.client()?;
        self.inner
            .wait_im(
                environment
                    .core
                    .device_revoke()
                    .revoke(im_core::identity::DeviceRevokeRequest {
                        identity: im_core::identity::IdentitySelector::Default,
                        target_device_id,
                        user_presence_confirmed: input.user_presence_confirmed,
                    }),
                self.inner.operation_timeout,
            )
            .await
            .map(crate::dto::device_revoke_result)
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
    pub async fn get_profile(&self) -> napi::Result<NodeProfile> {
        napi_result(self.get_profile_inner().await)
    }

    async fn get_profile_inner(&self) -> SafeResult<NodeProfile> {
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let profile = self
            .inner
            .wait_im(
                client.identity().profile_async(),
                self.inner.operation_timeout,
            )
            .await?;
        Ok(crate::dto::profile(profile))
    }

    #[napi(catch_unwind)]
    pub async fn update_profile(&self, input: NodeUpdateProfileInput) -> napi::Result<NodeProfile> {
        napi_result(self.update_profile_inner(input).await)
    }

    async fn update_profile_inner(&self, input: NodeUpdateProfileInput) -> SafeResult<NodeProfile> {
        let display_name = input.display_name.trim().to_owned();
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
                        bio: input
                            .bio
                            .map(|value| value.trim().to_owned())
                            .filter(|value| !value.is_empty()),
                        tags: input.tags.map(|tags| {
                            tags.into_iter()
                                .map(|tag| tag.trim().to_owned())
                                .filter(|tag| !tag.is_empty())
                                .collect()
                        }),
                        ..Default::default()
                    }),
                self.inner.operation_timeout,
            )
            .await?;
        environment.state.harden_permissions()?;
        Ok(crate::dto::profile(profile))
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
    pub async fn hydrate_display_profiles(
        &self,
        input: NodeDisplayProfileBatchInput,
    ) -> napi::Result<Vec<NodeDisplayProfile>> {
        napi_result(self.hydrate_display_profiles_inner(input).await)
    }

    async fn hydrate_display_profiles_inner(
        &self,
        input: NodeDisplayProfileBatchInput,
    ) -> SafeResult<Vec<NodeDisplayProfile>> {
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let request = crate::dto::display_profile_batch_request(input, client.did_domain())?;
        let profiles = self
            .inner
            .wait_im(
                client.directory().hydrate_display_profiles_async(request),
                self.inner.operation_timeout,
            )
            .await?;
        Ok(crate::dto::display_profiles(profiles))
    }

    #[napi(catch_unwind)]
    pub async fn create_group(&self, input: NodeCreateGroupInput) -> napi::Result<NodeGroup> {
        napi_result(self.create_group_inner(input).await)
    }

    async fn create_group_inner(&self, input: NodeCreateGroupInput) -> SafeResult<NodeGroup> {
        let _mutation = self.inner.mutation.lock().await;
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let request = crate::dto::group_create_request(input, client.handle())?;
        let fallback_title = request.name.clone();
        let result = self
            .inner
            .wait_im(
                client.groups().create_async(request),
                self.inner.operation_timeout,
            )
            .await?;
        crate::dto::created_group(result, &fallback_title)
    }

    #[napi(catch_unwind)]
    pub async fn add_group_member(
        &self,
        input: NodeAddGroupMemberInput,
    ) -> napi::Result<NodeGroupMember> {
        napi_result(self.add_group_member_inner(input).await)
    }

    async fn add_group_member_inner(
        &self,
        input: NodeAddGroupMemberInput,
    ) -> SafeResult<NodeGroupMember> {
        let _mutation = self.inner.mutation.lock().await;
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let request = crate::dto::group_member_mutation_request(input, client.did_domain())?;
        let result = self
            .inner
            .wait_im(
                client.groups().add_member_async(request),
                self.inner.operation_timeout,
            )
            .await?;
        crate::dto::added_group_member(result)
    }

    #[napi(catch_unwind)]
    pub async fn get_group(&self, input: NodeGroupInput) -> napi::Result<NodeGroup> {
        napi_result(self.get_group_inner(input).await)
    }

    async fn get_group_inner(&self, input: NodeGroupInput) -> SafeResult<NodeGroup> {
        let group = im_core::ids::GroupRef::parse(input.group_did).map_err(SafeError::from_im)?;
        let fallback = group.as_str().to_owned();
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let result = self
            .inner
            .wait_im(
                client.groups().get_async(group),
                self.inner.operation_timeout,
            )
            .await?;
        crate::dto::created_group(result, &fallback)
    }

    #[napi(catch_unwind)]
    pub async fn list_groups(
        &self,
        input: Option<NodePageInput>,
    ) -> napi::Result<NodePageOfGroups> {
        napi_result(self.list_groups_inner(input).await)
    }

    async fn list_groups_inner(
        &self,
        input: Option<NodePageInput>,
    ) -> SafeResult<NodePageOfGroups> {
        let input = input.unwrap_or(NodePageInput {
            cursor: None,
            limit: None,
        });
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let result = self
            .inner
            .wait_im(
                client
                    .groups()
                    .list_async(im_core::groups::GroupListRequest {
                        limit: page_limit(input.limit)?,
                        cursor: input
                            .cursor
                            .map(im_core::ids::Cursor::parse)
                            .transpose()
                            .map_err(SafeError::from_im)?,
                    }),
                self.inner.operation_timeout,
            )
            .await?;
        Ok(crate::dto::groups(result))
    }

    #[napi(catch_unwind)]
    pub async fn join_group(&self, input: NodeGroupInput) -> napi::Result<NodeGroup> {
        napi_result(self.join_group_inner(input).await)
    }

    async fn join_group_inner(&self, input: NodeGroupInput) -> SafeResult<NodeGroup> {
        let _mutation = self.inner.mutation.lock().await;
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let request = crate::dto::group_join_request(input, client.handle())?;
        let fallback = request.group.as_str().to_owned();
        let result = self
            .inner
            .wait_im(
                client.groups().join_async(request),
                self.inner.operation_timeout,
            )
            .await?;
        crate::dto::created_group(result, &fallback)
    }

    #[napi(catch_unwind)]
    pub async fn leave_group(&self, input: NodeGroupInput) -> napi::Result<()> {
        napi_result(self.leave_group_inner(input).await)
    }

    async fn leave_group_inner(&self, input: NodeGroupInput) -> SafeResult<()> {
        let group = im_core::ids::GroupRef::parse(input.group_did).map_err(SafeError::from_im)?;
        let _mutation = self.inner.mutation.lock().await;
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        self.inner
            .wait_im(
                client
                    .groups()
                    .leave_async(im_core::groups::GroupLeaveRequest {
                        group,
                        reason_text: None,
                        security: im_core::groups::GroupSecurityRequirement::default(),
                    }),
                self.inner.operation_timeout,
            )
            .await?;
        Ok(())
    }

    #[napi(catch_unwind)]
    pub async fn list_group_members(
        &self,
        input: NodeGroupMembersInput,
    ) -> napi::Result<NodeGroupMemberPage> {
        napi_result(self.list_group_members_inner(input).await)
    }

    async fn list_group_members_inner(
        &self,
        input: NodeGroupMembersInput,
    ) -> SafeResult<NodeGroupMemberPage> {
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let result = self
            .inner
            .wait_im(
                client
                    .groups()
                    .members_async(im_core::groups::GroupMembersRequest {
                        group: im_core::ids::GroupRef::parse(input.group_did)
                            .map_err(SafeError::from_im)?,
                        limit: page_limit(input.limit)?,
                        cursor: input
                            .cursor
                            .map(im_core::ids::Cursor::parse)
                            .transpose()
                            .map_err(SafeError::from_im)?,
                    }),
                self.inner.operation_timeout,
            )
            .await?;
        Ok(crate::dto::group_member_page(result))
    }

    #[napi(catch_unwind)]
    pub async fn remove_group_member(
        &self,
        input: NodeRemoveGroupMemberInput,
    ) -> napi::Result<NodeGroupMember> {
        napi_result(self.remove_group_member_inner(input).await)
    }

    async fn remove_group_member_inner(
        &self,
        input: NodeRemoveGroupMemberInput,
    ) -> SafeResult<NodeGroupMember> {
        let _mutation = self.inner.mutation.lock().await;
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let result = self
            .inner
            .wait_im(
                client
                    .groups()
                    .remove_member_async(crate::dto::group_member_mutation_request(
                        NodeAddGroupMemberInput {
                            group_did: input.group_did,
                            member: input.member,
                            role: None,
                        },
                        client.did_domain(),
                    )?),
                self.inner.operation_timeout,
            )
            .await?;
        crate::dto::added_group_member(result)
    }

    #[napi(catch_unwind)]
    pub async fn resume_group_rebind_recovery(
        &self,
        limit: Option<u32>,
    ) -> napi::Result<NodeGroupRebindRecoverySummary> {
        napi_result(self.resume_group_rebind_recovery_inner(limit).await)
    }

    async fn resume_group_rebind_recovery_inner(
        &self,
        limit: Option<u32>,
    ) -> SafeResult<NodeGroupRebindRecoverySummary> {
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let value = self
            .inner
            .wait_im(
                client
                    .groups()
                    .resume_rebind_recovery_async(limit.unwrap_or(100)),
                self.inner.operation_timeout,
            )
            .await?;
        Ok(crate::dto::rebind_recovery_summary(value))
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
    pub async fn start_realtime(
        &self,
        input: Option<NodeRealtimeOptions>,
    ) -> napi::Result<NativeRealtimeSession> {
        napi_result(self.start_realtime_inner(input).await)
    }

    async fn start_realtime_inner(
        &self,
        input: Option<NodeRealtimeOptions>,
    ) -> SafeResult<NativeRealtimeSession> {
        let _mutation = self.inner.mutation.lock().await;
        let mut realtime = self.inner.realtime.lock().await;
        if realtime.is_some() {
            return Err(SafeError::new(
                "realtime_already_started",
                "A realtime session is already active for this client.",
                false,
            ));
        }
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let options = realtime_options(input)?;
        let mut session = self
            .inner
            .wait_im(
                client.realtime().start_async(options),
                self.inner.operation_timeout,
            )
            .await?;
        let events = session.subscribe().map_err(SafeError::from_im)?;
        let status = session.status_updates();
        let id = self.inner.next_realtime_id.fetch_add(1, Ordering::Relaxed);
        let reader = Arc::new(tokio::sync::Mutex::new(RealtimeEventReader {
            events,
            pending: VecDeque::new(),
            connected_once: false,
        }));
        *realtime = Some(RealtimeSlot { id, session });
        Ok(NativeRealtimeSession {
            id,
            client: self.inner.clone(),
            events: reader,
            status,
        })
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
                        reason: LIST_CONVERSATIONS_SYNC_REASON.to_owned(),
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
                client.messages().local_conversation_timeline_async(
                    conversation,
                    im_core::messages::LocalHistoryQuery {
                        limit: page_limit(input.limit)?,
                        cursor: input
                            .cursor
                            .map(im_core::ids::Cursor::parse)
                            .transpose()
                            .map_err(SafeError::from_im)?,
                    },
                ),
                self.inner.operation_timeout,
            )
            .await?;
        crate::dto::messages(page, &input.conversation_id)
    }

    #[napi(catch_unwind)]
    pub async fn get_local_conversation_timeline(
        &self,
        input: NodeHistoryInput,
    ) -> napi::Result<NodePageOfMessages> {
        napi_result(self.get_local_conversation_timeline_inner(input).await)
    }

    async fn get_local_conversation_timeline_inner(
        &self,
        input: NodeHistoryInput,
    ) -> SafeResult<NodePageOfMessages> {
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let conversation = im_core::messages::ConversationReadRef::new(&input.conversation_id)
            .map_err(SafeError::from_im)?;
        let page = self
            .inner
            .wait_im(
                client.messages().local_conversation_timeline_async(
                    conversation,
                    im_core::messages::LocalHistoryQuery {
                        limit: page_limit(input.limit)?,
                        cursor: input
                            .cursor
                            .map(im_core::ids::Cursor::parse)
                            .transpose()
                            .map_err(SafeError::from_im)?,
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
        let messages = client.messages();
        let send = box_im_future(
            messages.send_conversation_text_async(im_core::messages::SendConversationTextRequest {
                conversation: im_core::messages::ConversationReadRef::new(&conversation_id)
                    .map_err(SafeError::from_im)?,
                text: input.text,
                markdown: input.markdown.unwrap_or(false),
                security: im_core::messages::MessageSecurityMode::DefaultPlain,
                client_message_id: optional_message_id(input.client_message_id)?,
                idempotency_key: non_empty_optional(input.idempotency_key),
                wait_for_final_acceptance: false,
                delegated_signing: None,
            }),
        );
        let result = self
            .inner
            .wait_im(send, self.inner.operation_timeout)
            .await?;
        crate::dto::sent_message(result.message, &conversation_id)
    }

    #[napi(catch_unwind)]
    pub async fn send_payload(&self, input: NodeSendPayloadInput) -> napi::Result<NodeMessage> {
        napi_result(self.send_payload_inner(input).await)
    }

    async fn send_payload_inner(&self, input: NodeSendPayloadInput) -> SafeResult<NodeMessage> {
        let payload: serde_json::Value =
            serde_json::from_str(&input.payload_json).map_err(|_| {
                SafeError::new("invalid_input", "The message payload is invalid.", false)
            })?;
        if im_core::messages::is_message_mention_payload(&payload) {
            im_core::messages::validate_message_mention_payload(&payload)
                .map_err(SafeError::from_im)?;
        }
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let conversation_id = input.conversation_id;
        let messages = client.messages();
        let send = box_im_future(
            messages.send_conversation_payload_async(
                im_core::messages::SendConversationPayloadRequest {
                    conversation: im_core::messages::ConversationReadRef::new(&conversation_id)
                        .map_err(SafeError::from_im)?,
                    payload,
                    security: im_core::messages::MessageSecurityMode::DefaultPlain,
                    client_message_id: optional_message_id(input.client_message_id)?,
                    idempotency_key: non_empty_optional(input.idempotency_key),
                    wait_for_final_acceptance: false,
                    delegated_signing: None,
                },
            ),
        );
        let result = self
            .inner
            .wait_im(send, self.inner.operation_timeout)
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
        let attachments = client.attachments();
        let send = box_im_future(
            attachments.send_conversation_async(
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
        );
        let result = self
            .inner
            .wait_im(send, self.inner.operation_timeout)
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
        let attachments = client.attachments();
        let conversation = im_core::messages::ConversationReadRef::new(input.conversation_id)
            .map_err(SafeError::from_im)?;
        let download = box_im_future(attachments.download_async(
            im_core::attachments::DownloadAttachmentRequest {
                thread: conversation.as_thread_ref().map_err(SafeError::from_im)?,
                message_id:
                    im_core::ids::MessageId::parse(input.message_id).map_err(SafeError::from_im)?,
                attachment_id: non_empty_optional(input.attachment_id),
                destination: im_core::attachments::AttachmentDestination::Memory,
                overwrite: false,
            },
        ));
        let result = self.inner.wait_im(download, timeout).await?;
        crate::dto::downloaded_attachment(result)
    }

    #[napi(catch_unwind)]
    pub async fn get_mail_account(&self) -> napi::Result<NodeMailAccount> {
        napi_result(self.get_mail_account_inner().await)
    }

    async fn get_mail_account_inner(&self) -> SafeResult<NodeMailAccount> {
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let result = self
            .inner
            .wait_im(client.email().account_async(), self.inner.operation_timeout)
            .await?;
        crate::dto::mail_account(result)
    }

    #[napi(catch_unwind)]
    pub async fn list_mail_inbox(
        &self,
        input: Option<NodeMailInboxInput>,
    ) -> napi::Result<NodeMailInboxPage> {
        napi_result(self.list_mail_inbox_inner(input).await)
    }

    async fn list_mail_inbox_inner(
        &self,
        input: Option<NodeMailInboxInput>,
    ) -> SafeResult<NodeMailInboxPage> {
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let (query, offset, limit) = mail_inbox_query(input)?;
        let result = self
            .inner
            .wait_im(
                client.email().inbox_async(query),
                self.inner.operation_timeout,
            )
            .await?;
        crate::dto::mail_inbox(result, offset, limit)
    }

    #[napi(catch_unwind)]
    pub async fn read_mail(&self, message_id: String) -> napi::Result<NodeMailMessage> {
        napi_result(self.read_mail_inner(message_id).await)
    }

    async fn read_mail_inner(&self, message_id: String) -> SafeResult<NodeMailMessage> {
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let message_id = mail_message_id(message_id)?;
        let result = self
            .inner
            .wait_im(
                client.email().read_async(message_id),
                self.inner.operation_timeout,
            )
            .await?;
        crate::dto::mail_message(result)
    }

    #[napi(catch_unwind)]
    pub async fn mark_mail_read(
        &self,
        input: NodeMarkMailReadInput,
    ) -> napi::Result<NodeMarkMailReadResult> {
        napi_result(self.mark_mail_read_inner(input).await)
    }

    async fn mark_mail_read_inner(
        &self,
        input: NodeMarkMailReadInput,
    ) -> SafeResult<NodeMarkMailReadResult> {
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let request = mark_mail_read_request(input)?;
        let result = self
            .inner
            .wait_im(
                client.email().mark_read_async(request),
                self.inner.operation_timeout,
            )
            .await?;
        Ok(crate::dto::mark_mail_read_result(result))
    }

    #[napi(catch_unwind)]
    pub async fn send_mail(&self, input: NodeSendMailInput) -> napi::Result<NodeSendMailResult> {
        napi_result(self.send_mail_inner(input).await)
    }

    async fn send_mail_inner(&self, input: NodeSendMailInput) -> SafeResult<NodeSendMailResult> {
        let operation = self.inner.operation().await?;
        let client = operation.client()?;
        let request = send_mail_request(input)?;
        let email = client.email();
        let send = box_im_future(email.send_async(request));
        let result = self
            .inner
            .wait_im(send, self.inner.operation_timeout)
            .await?;
        crate::dto::send_mail_result(result)
    }

    #[napi(catch_unwind)]
    pub async fn request_handle_recovery_otp(
        &self,
        input: NodeHandleRecoveryOtpInput,
    ) -> napi::Result<NodeHandleRecoveryOtpResult> {
        napi_result(self.request_handle_recovery_otp_inner(input).await)
    }

    async fn request_handle_recovery_otp_inner(
        &self,
        input: NodeHandleRecoveryOtpInput,
    ) -> SafeResult<NodeHandleRecoveryOtpResult> {
        let _mutation = self.inner.mutation.lock().await;
        let operation = self.inner.operation().await?;
        let environment = operation.environment()?;
        let value = self
            .inner
            .wait_im(
                environment
                    .core
                    .handle_recovery()
                    .request_handle_recovery_otp(im_core::identity::HandleRecoveryOtpRequest {
                        identity: None,
                        full_handle: input.full_handle,
                        phone: input.phone,
                    }),
                self.inner.operation_timeout,
            )
            .await?;
        environment.state.harden_permissions()?;
        Ok(crate::dto::recovery_otp_result(value))
    }

    #[napi(catch_unwind)]
    pub async fn prepare_handle_recovery(
        &self,
        input: NodeHandleRecoveryPrepareInput,
    ) -> napi::Result<NodeHandleRecoveryProgress> {
        napi_result(self.prepare_handle_recovery_inner(input).await)
    }

    async fn prepare_handle_recovery_inner(
        &self,
        input: NodeHandleRecoveryPrepareInput,
    ) -> SafeResult<NodeHandleRecoveryProgress> {
        let _mutation = self.inner.mutation.lock().await;
        let operation = self.inner.operation().await?;
        let environment = operation.environment()?;
        let value = self
            .inner
            .wait_im(
                environment.core.handle_recovery().prepare_handle_recovery(
                    im_core::identity::HandleRecoveryPrepareRequest {
                        operation_id: input.operation_id,
                        phone: input.phone,
                        code: input.otp,
                    },
                ),
                self.inner.operation_timeout,
            )
            .await?;
        environment.state.harden_permissions()?;
        Ok(crate::dto::recovery_progress(value))
    }

    #[napi(catch_unwind)]
    pub async fn activate_handle_recovery(
        &self,
        input: NodeHandleRecoveryOperationInput,
    ) -> napi::Result<NodeHandleRecoveryProgress> {
        napi_result(self.activate_handle_recovery_inner(input).await)
    }

    async fn activate_handle_recovery_inner(
        &self,
        input: NodeHandleRecoveryOperationInput,
    ) -> SafeResult<NodeHandleRecoveryProgress> {
        let _mutation = self.inner.mutation.lock().await;
        let mut operation = self.inner.write_operation().await?;
        let environment = operation.as_mut().ok_or_else(SafeError::closed)?;
        let value = self
            .inner
            .wait_im(
                environment.core.handle_recovery().activate_handle_recovery(
                    im_core::identity::HandleRecoveryActivateRequest {
                        operation_id: input.operation_id,
                        user_presence_confirmed: true,
                    },
                ),
                self.inner.operation_timeout,
            )
            .await?;
        self.refresh_recovered_client(environment, &value).await?;
        environment.state.harden_permissions()?;
        Ok(crate::dto::recovery_progress(value))
    }

    #[napi(catch_unwind)]
    pub async fn get_handle_recovery_status(
        &self,
        input: NodeHandleRecoveryOperationInput,
    ) -> napi::Result<NodeHandleRecoveryProgress> {
        napi_result(self.get_handle_recovery_status_inner(input).await)
    }

    async fn get_handle_recovery_status_inner(
        &self,
        input: NodeHandleRecoveryOperationInput,
    ) -> SafeResult<NodeHandleRecoveryProgress> {
        let operation = self.inner.operation().await?;
        let environment = operation.environment()?;
        let value = environment
            .core
            .handle_recovery()
            .handle_recovery_status(&input.operation_id)
            .map_err(SafeError::from_im)?;
        Ok(crate::dto::recovery_progress(value))
    }

    #[napi(catch_unwind)]
    pub async fn resume_handle_recovery(
        &self,
        input: NodeHandleRecoveryOperationInput,
    ) -> napi::Result<NodeHandleRecoveryProgress> {
        napi_result(self.resume_handle_recovery_inner(input).await)
    }

    #[napi(catch_unwind)]
    pub async fn issue_handle_recovery_attestation(
        &self,
        input: NodeHandleRecoveryOperationInput,
    ) -> napi::Result<NodeHandleRecoveryAttestationResult> {
        napi_result(self.issue_handle_recovery_attestation_inner(input).await)
    }

    async fn issue_handle_recovery_attestation_inner(
        &self,
        input: NodeHandleRecoveryOperationInput,
    ) -> SafeResult<NodeHandleRecoveryAttestationResult> {
        let operation = self.inner.operation().await?;
        let environment = operation.environment()?;
        let value = self
            .inner
            .wait_im(
                environment
                    .core
                    .handle_recovery()
                    .issue_handle_recovery_attestation(
                        im_core::identity::HandleRecoveryAttestationRequest {
                            operation_id: input.operation_id,
                        },
                    ),
                self.inner.operation_timeout,
            )
            .await?;
        Ok(crate::dto::recovery_attestation(value))
    }

    async fn resume_handle_recovery_inner(
        &self,
        input: NodeHandleRecoveryOperationInput,
    ) -> SafeResult<NodeHandleRecoveryProgress> {
        let _mutation = self.inner.mutation.lock().await;
        let mut operation = self.inner.write_operation().await?;
        let environment = operation.as_mut().ok_or_else(SafeError::closed)?;
        let value = self
            .inner
            .wait_im(
                environment.core.handle_recovery().resume_handle_recovery(
                    im_core::identity::HandleRecoveryResumeRequest {
                        operation_id: input.operation_id,
                    },
                ),
                self.inner.operation_timeout,
            )
            .await?;
        self.refresh_recovered_client(environment, &value).await?;
        environment.state.harden_permissions()?;
        Ok(crate::dto::recovery_progress(value))
    }

    #[napi(catch_unwind)]
    pub async fn discard_handle_recovery(
        &self,
        input: NodeHandleRecoveryOperationInput,
    ) -> napi::Result<NodeHandleRecoveryOperationSummary> {
        napi_result(self.discard_handle_recovery_inner(input).await)
    }

    async fn discard_handle_recovery_inner(
        &self,
        input: NodeHandleRecoveryOperationInput,
    ) -> SafeResult<NodeHandleRecoveryOperationSummary> {
        let _mutation = self.inner.mutation.lock().await;
        let operation = self.inner.operation().await?;
        let environment = operation.environment()?;
        let value = environment
            .core
            .handle_recovery()
            .discard_handle_recovery_pre_attempt(im_core::identity::HandleRecoveryDiscardRequest {
                operation_id: input.operation_id,
            })
            .await
            .map_err(SafeError::from_im)?;
        environment.state.harden_permissions()?;
        Ok(crate::dto::recovery_operation_summary(value))
    }

    async fn refresh_recovered_client(
        &self,
        environment: &mut Environment,
        progress: &im_core::identity::HandleRecoveryProgress,
    ) -> SafeResult<()> {
        if progress.phase != im_core::identity::HandleRecoveryPhase::Applied {
            return Ok(());
        }
        let client = self
            .inner
            .wait_im(
                environment
                    .core
                    .client_async(im_core::identity::IdentitySelector::Id(
                        progress.owner_identity_id.clone(),
                    )),
                self.inner.operation_timeout,
            )
            .await?;
        self.inner
            .wait_im(
                client.groups().resume_rebind_recovery_async(100),
                self.inner.operation_timeout,
            )
            .await?;
        environment.client = Some(client);
        Ok(())
    }

    #[napi(catch_unwind)]
    pub async fn close(&self) -> napi::Result<()> {
        napi_result(self.inner.close().await)
    }

    #[napi(catch_unwind)]
    pub async fn clear_local_data(&self) -> napi::Result<NodeClearLocalDataResult> {
        napi_result(self.clear_local_data_inner().await)
    }

    async fn clear_local_data_inner(&self) -> SafeResult<NodeClearLocalDataResult> {
        let _mutation = self.inner.mutation.lock().await;
        self.inner.stop_realtime(None).await?;
        let mut slot = self.inner.write_operation().await?;
        let environment = slot.take().ok_or_else(SafeError::closed)?;
        let Environment {
            core,
            client,
            state,
        } = environment;
        drop(client);
        drop(core);

        let (state, cleared) = self
            .inner
            .wait_safe(
                async move {
                    tokio::task::spawn_blocking(move || {
                        let cleared = state.clear_owned_data()?;
                        Ok((state, cleared))
                    })
                    .await
                    .map_err(|_| SafeError::internal())?
                },
                self.inner.operation_timeout,
            )
            .await?;
        let identity_provider = self
            .inner
            .identity_provider
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        *slot = Some(
            initialize_environment(
                self.inner.options.clone(),
                state,
                identity_provider,
                self.inner.operation_timeout,
            )
            .await?,
        );
        Ok(NodeClearLocalDataResult { cleared })
    }
}

impl Drop for NativeImCoreNodeClient {
    fn drop(&mut self) {
        self.inner.cancel_from_drop();
    }
}

pub(crate) async fn open(
    options: NodeOpenOptions,
    identity_provider_dispatch: Option<crate::external_identity::IdentityProviderDispatch>,
) -> SafeResult<NativeImCoreNodeClient> {
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
    let identity_provider =
        identity_provider_dispatch.map(crate::external_identity::ExternalIdentityCustody::new);
    let environment = initialize_environment(
        options.clone(),
        state,
        identity_provider.clone(),
        operation_timeout,
    )
    .await?;
    Ok(NativeImCoreNodeClient {
        inner: Arc::new(ClientInner {
            lifecycle: AtomicU8::new(LIFECYCLE_OPEN),
            environment: RwLock::new(Some(environment)),
            mutation: tokio::sync::Mutex::new(()),
            cancellation: CancellationToken::new(),
            closed: Notify::new(),
            operation_timeout,
            sync_timeout,
            options,
            identity_provider: std::sync::Mutex::new(identity_provider),
            realtime: tokio::sync::Mutex::new(None),
            next_realtime_id: AtomicU64::new(1),
            #[cfg(test)]
            realtime_close_error: std::sync::Mutex::new(None),
        }),
    })
}

async fn initialize_environment(
    options: NodeOpenOptions,
    state: StateRoot,
    identity_provider: Option<crate::external_identity::ExternalIdentityCustody>,
    operation_timeout: Duration,
) -> SafeResult<Environment> {
    if let Some(provider) = identity_provider.as_ref() {
        tokio::time::timeout(operation_timeout, provider.handshake())
            .await
            .map_err(|_| SafeError::timeout())??;
    }
    let paths = state.paths();
    let config = core_config(&options)?;
    let core = im_core::ImCore::open_with_options(
        config,
        paths,
        core_open_options(&options, &state, identity_provider)?,
    )
    .await
    .map_err(SafeError::from_im)?;
    core.bootstrap()
        .initialize_local_state_async()
        .await
        .map_err(SafeError::from_im)?;
    state.harden_permissions()?;
    let default_identity = core
        .identities()
        .default_identity_async()
        .await
        .map_err(SafeError::from_im)?;
    if let Some(identity) = default_identity.as_ref() {
        migrate_legacy_identity_to_anp(&core, identity).await?;
    }
    let client = match default_identity {
        Some(_) => Some(
            core.client_async(im_core::identity::IdentitySelector::Default)
                .await
                .map_err(SafeError::from_im)?,
        ),
        None => None,
    };
    Ok(Environment {
        core,
        client,
        state,
    })
}

fn core_open_options(
    options: &NodeOpenOptions,
    state: &StateRoot,
    identity_provider: Option<crate::external_identity::ExternalIdentityCustody>,
) -> SafeResult<im_core::ImCoreOpenOptions> {
    let core_options = im_core::ImCoreOpenOptions::default()
        .with_identity_secret_vault(
            im_core::IdentitySecretStoragePolicy::VaultRequired,
            state.identity_vault_options()?,
        )
        .with_multi_device_device_revoke_enabled(
            options.multi_device_device_revoke_enabled.unwrap_or(false),
        )
        .with_multi_device_handle_recovery_enabled(
            options
                .multi_device_handle_recovery_enabled
                .unwrap_or(false),
        )
        .with_external_http_allow_insecure_loopback_for_testing(
            options
                .external_http_allow_insecure_loopback_for_testing
                .unwrap_or(false),
        );
    let core_options = match &options.multi_device_audience {
        Some(audience) => core_options.with_multi_device_audience(audience.clone()),
        None => core_options,
    };
    Ok(match identity_provider {
        Some(provider) => core_options.with_identity_custody_provider(Arc::new(provider)),
        None => core_options,
    })
}

pub(crate) fn core_config(options: &NodeOpenOptions) -> SafeResult<im_core::ImCoreConfig> {
    let mut config = im_core::ImCoreConfig::new(
        im_core::ServiceEndpoint::parse(&options.service_base_url).map_err(SafeError::from_im)?,
        options.did_domain.clone(),
    )
    .map_err(SafeError::from_im)?;
    config.user_service_endpoint = optional_endpoint(options.user_service_endpoint.clone())?;
    config.message_service_endpoint = optional_endpoint(options.message_service_endpoint.clone())?;
    config.mail_service_endpoint = optional_endpoint(options.mail_service_endpoint.clone())?;
    config.anp_service_endpoint = optional_endpoint(options.anp_service_endpoint.clone())?;
    config.anp_service_did = options
        .anp_service_did
        .clone()
        .map(im_core::ids::Did::parse)
        .transpose()
        .map_err(SafeError::from_im)?;
    config.transport_policy = im_core::MessageTransportPolicy::RealtimePreferred;
    Ok(config)
}

async fn migrate_legacy_identity_to_anp(
    core: &im_core::ImCore,
    identity: &im_core::identity::IdentitySummary,
) -> SafeResult<()> {
    let registry = core.identities();
    let selector = im_core::identity::IdentitySelector::Default;
    let status = registry
        .custody_status_async(selector.clone())
        .await
        .map_err(SafeError::from_im)?;
    if status.backend == im_core::identity::IdentityCustodyBackend::AnpIdentity && status.ready {
        return Ok(());
    }
    let migration = registry
        .migrate_identity_custody_async()
        .await
        .map_err(SafeError::from_im)?;
    if migration.phase == im_core::identity::IdentityCustodyMigrationPhase::Blocked
        || !migration.blockers.is_empty()
    {
        return Err(identity_migration_failed());
    }
    let after = registry
        .custody_status_async(selector)
        .await
        .map_err(SafeError::from_im)?;
    ensure_identity_preserved(
        identity,
        &after.identity,
        after.backend == im_core::identity::IdentityCustodyBackend::AnpIdentity
            && after.ready
            && after.missing.is_empty(),
    )
}

fn ensure_identity_preserved(
    before: &im_core::identity::IdentitySummary,
    after: &im_core::identity::IdentitySummary,
    verified: bool,
) -> SafeResult<()> {
    if !verified
        || after.id != before.id
        || after.did != before.did
        || after.handle != before.handle
    {
        return Err(identity_migration_failed());
    }
    Ok(())
}

fn identity_migration_failed() -> SafeError {
    SafeError::new(
        "identity_migration_failed",
        "The existing IM identity could not be preserved during secure storage migration.",
        false,
    )
}

fn realtime_options(
    input: Option<NodeRealtimeOptions>,
) -> SafeResult<im_core::realtime::RealtimeOptions> {
    let input = input.unwrap_or(NodeRealtimeOptions {
        event_buffer: None,
        reconnect_base_delay_ms: None,
        reconnect_max_delay_ms: None,
        reconnect_max_attempts: None,
    });
    let event_buffer = input.event_buffer.unwrap_or(DEFAULT_REALTIME_EVENT_BUFFER);
    let base_delay_ms = input
        .reconnect_base_delay_ms
        .unwrap_or(DEFAULT_RECONNECT_BASE_DELAY_MS);
    let max_delay_ms = input
        .reconnect_max_delay_ms
        .unwrap_or(DEFAULT_RECONNECT_MAX_DELAY_MS);
    if event_buffer == 0
        || event_buffer > MAX_REALTIME_EVENT_BUFFER
        || base_delay_ms == 0
        || max_delay_ms == 0
        || base_delay_ms > max_delay_ms
        || max_delay_ms > MAX_TIMEOUT_MS
    {
        return Err(SafeError::new(
            "invalid_input",
            "The realtime options are invalid.",
            false,
        ));
    }
    Ok(im_core::realtime::RealtimeOptions {
        reconnect: im_core::realtime::ReconnectPolicy::Exponential {
            base_delay_ms: u64::from(base_delay_ms),
            max_delay_ms: u64::from(max_delay_ms),
            max_attempts: input.reconnect_max_attempts,
        },
        event_buffer: usize::try_from(event_buffer).map_err(|_| SafeError::internal())?,
        subscriptions: vec![im_core::realtime::RealtimeSubscription::Messages],
    })
}

fn node_realtime_status(status: im_core::realtime::RealtimeStatus) -> NodeRealtimeStatus {
    NodeRealtimeStatus {
        connected: status.connected,
        state: realtime_state(status.state).to_owned(),
    }
}

fn realtime_state(state: im_core::realtime::RealtimeConnectionState) -> &'static str {
    match state {
        im_core::realtime::RealtimeConnectionState::Disconnected => "disconnected",
        im_core::realtime::RealtimeConnectionState::Connecting => "connecting",
        im_core::realtime::RealtimeConnectionState::Connected => "connected",
        im_core::realtime::RealtimeConnectionState::Reconnecting => "reconnecting",
        im_core::realtime::RealtimeConnectionState::Closed => "closed",
    }
}

fn map_realtime_event(
    event: im_core::realtime::ImEvent,
    connected_once: bool,
) -> Vec<NodeRealtimeEvent> {
    use im_core::realtime::ImEvent;

    match event {
        ImEvent::ConnectionStateChanged(change) => {
            let state = realtime_state(change.state).to_owned();
            let mut mapped = vec![NodeRealtimeEvent {
                kind: "connection_state_changed".to_owned(),
                state: Some(state.clone()),
                cause: None,
                dirty: None,
                gap_detected: None,
            }];
            if state == "connected" {
                mapped.push(sync_required_event(
                    if connected_once {
                        "reconnected"
                    } else {
                        "connection_ready"
                    },
                    false,
                    false,
                ));
            }
            mapped
        }
        ImEvent::MessageReceived(event) => vec![sync_required_from_hint("message", event.sync)],
        ImEvent::MessageUpdated(event) => {
            vec![sync_required_from_hint("message_update", event.sync)]
        }
        ImEvent::GroupUpdated(event) => vec![sync_required_from_hint("group", event.sync)],
        ImEvent::SystemNotificationChanged(event) => {
            vec![sync_required_from_hint("system_notification", event.sync)]
        }
        ImEvent::UnknownNotification(event) => {
            vec![sync_required_from_hint("stream_recovery", event.sync)]
        }
        ImEvent::LocalNotification(_) | ImEvent::HostNotification(_) => Vec::new(),
    }
}

fn sync_required_from_hint(
    cause: &str,
    hint: Option<im_core::realtime::RealtimeSyncHint>,
) -> NodeRealtimeEvent {
    let (dirty, gap_detected) = hint
        .map(|hint| {
            (
                hint.sync_dirty || hint.has_unknown_domain,
                hint.gap_detected,
            )
        })
        .unwrap_or((false, false));
    sync_required_event(cause, dirty, gap_detected)
}

fn sync_required_event(cause: &str, dirty: bool, gap_detected: bool) -> NodeRealtimeEvent {
    NodeRealtimeEvent {
        kind: "sync_required".to_owned(),
        state: None,
        cause: Some(cause.to_owned()),
        dirty: Some(dirty),
        gap_detected: Some(gap_detected),
    }
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

fn local_new_device_session(
    environment: &Environment,
    join_session_id: &str,
) -> SafeResult<Option<im_core::identity::DeviceJoinSessionView>> {
    environment
        .core
        .device_join()
        .local_sessions()
        .map(|sessions| {
            sessions.into_iter().find(|session| {
                session.side == im_core::identity::DeviceJoinSide::NewDevice
                    && session.join_session_id == join_session_id
            })
        })
        .map_err(SafeError::from_im)
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

pub(crate) fn mail_inbox_query(
    input: Option<NodeMailInboxInput>,
) -> SafeResult<(im_core::email::EmailInboxQuery, u32, u32)> {
    let input = input.unwrap_or(NodeMailInboxInput {
        folder: None,
        limit: None,
        offset: None,
        unread_only: None,
    });
    let folder = input.folder.unwrap_or_else(|| "inbox".to_owned());
    validate_mail_token(&folder, 64)?;
    let limit = input.limit.unwrap_or(DEFAULT_MAIL_INBOX_LIMIT);
    if !(1..=MAX_MAIL_INBOX_LIMIT).contains(&limit) {
        return Err(invalid_input("The mail inbox limit is invalid."));
    }
    let offset = input.offset.unwrap_or_default();
    Ok((
        im_core::email::EmailInboxQuery {
            folder: im_core::email::EmailFolder::parse(folder).map_err(SafeError::from_im)?,
            limit: im_core::ids::PageLimit::new(limit).map_err(SafeError::from_im)?,
            offset,
            unread_only: input.unread_only.unwrap_or(false),
        },
        offset,
        limit,
    ))
}

pub(crate) fn mail_message_id(value: String) -> SafeResult<im_core::email::EmailMessageId> {
    validate_mail_token(&value, 2_048)?;
    im_core::email::EmailMessageId::parse(value).map_err(SafeError::from_im)
}

pub(crate) fn mark_mail_read_request(
    input: NodeMarkMailReadInput,
) -> SafeResult<im_core::email::EmailMarkReadRequest> {
    if input.message_ids.is_empty() || input.message_ids.len() > MAX_MAIL_MESSAGE_IDS {
        return Err(invalid_input("The mail message ID collection is invalid."));
    }
    Ok(im_core::email::EmailMarkReadRequest {
        message_ids: input
            .message_ids
            .into_iter()
            .map(mail_message_id)
            .collect::<SafeResult<Vec<_>>>()?,
        is_read: true,
    })
}

pub(crate) fn send_mail_request(
    input: NodeSendMailInput,
) -> SafeResult<im_core::email::SendEmailRequest> {
    let cc = input.cc.unwrap_or_default();
    if input.to.is_empty()
        || input.to.len() > MAX_MAIL_RECIPIENTS
        || input.to.len().saturating_add(cc.len()) > MAX_MAIL_RECIPIENTS
    {
        return Err(invalid_input("The mail recipient collection is invalid."));
    }
    let mut recipients = HashSet::with_capacity(input.to.len() + cc.len());
    for address in input.to.iter().chain(&cc) {
        validate_mail_address(address)?;
        if !recipients.insert(address.as_str()) {
            return Err(invalid_input(
                "The mail recipient collection has duplicates.",
            ));
        }
    }
    if input.subject.trim() != input.subject
        || input.subject.is_empty()
        || input.subject.len() > 1_024
    {
        return Err(invalid_input("The mail subject is invalid."));
    }
    if input.body_text.trim().is_empty() || input.body_text.len() > 65_536 {
        return Err(invalid_input("The mail body is invalid."));
    }
    Ok(im_core::email::SendEmailRequest {
        to: input
            .to
            .into_iter()
            .map(im_core::email::EmailAddress::parse)
            .collect::<im_core::ImResult<Vec<_>>>()
            .map_err(SafeError::from_im)?,
        cc: cc
            .into_iter()
            .map(im_core::email::EmailAddress::parse)
            .collect::<im_core::ImResult<Vec<_>>>()
            .map_err(SafeError::from_im)?,
        subject: input.subject,
        body_text: input.body_text,
        body_html: None,
    })
}

fn validate_mail_token(value: &str, max_chars: usize) -> SafeResult<()> {
    if value.is_empty()
        || value.trim() != value
        || value.chars().count() > max_chars
        || value.chars().any(char::is_control)
    {
        return Err(invalid_input("The mail request identifier is invalid."));
    }
    Ok(())
}

fn validate_mail_address(value: &str) -> SafeResult<()> {
    if !(3..=320).contains(&value.chars().count())
        || !value.contains('@')
        || value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(invalid_input("The mail address is invalid."));
    }
    Ok(())
}

fn invalid_input(message: &'static str) -> SafeError {
    SafeError::new("invalid_input", message, false)
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
                .unwrap_or_else(|| "manual_refresh".to_owned()),
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
            mail_service_endpoint: None,
            anp_service_endpoint: None,
            anp_service_did: None,
            operation_timeout_ms: Some(1_000),
            sync_timeout_ms: Some(100),
            multi_device_device_revoke_enabled: None,
            multi_device_handle_recovery_enabled: None,
            multi_device_audience: None,
            external_http_allow_insecure_loopback_for_testing: None,
        }
    }

    #[tokio::test]
    async fn close_is_idempotent_rejects_new_work_and_releases_the_state_lock() {
        let directory = tempfile::tempdir().unwrap();
        let client = open(options(directory.path()), None).await.unwrap();
        client.inner.close().await.unwrap();
        client.inner.close().await.unwrap();
        assert_eq!(
            client.get_default_identity_inner().await.unwrap_err().code,
            "client_closed"
        );
        assert_eq!(
            client
                .get_local_conversation_timeline_inner(NodeHistoryInput {
                    conversation_id: "dm:did:example:bob".to_owned(),
                    cursor: None,
                    limit: None,
                })
                .await
                .unwrap_err()
                .code,
            "client_closed"
        );
        open(options(directory.path()), None).await.unwrap();
    }

    #[tokio::test]
    async fn external_http_auth_requires_identity_and_enforces_body_limit() {
        let directory = tempfile::tempdir().unwrap();
        let client = open(options(directory.path()), None).await.unwrap();

        let error = client
            .prepare_external_http_request_inner(NodeExternalHttpRequest {
                url: "https://api.example.test/orders".to_owned(),
                method: "POST".to_owned(),
                headers: vec![NodeExternalHttpHeader {
                    name: "content-type".to_owned(),
                    value: "application/json".to_owned(),
                }],
                body: Some(Vec::new().into()),
            })
            .await
            .unwrap_err();
        assert_eq!(error.code, "identity_required");

        let error = client
            .prepare_external_http_request_inner(NodeExternalHttpRequest {
                url: "https://api.example.test/orders".to_owned(),
                method: "POST".to_owned(),
                headers: Vec::new(),
                body: Some(vec![0; im_core::EXTERNAL_HTTP_AUTH_MAX_BODY_BYTES + 1].into()),
            })
            .await
            .unwrap_err();
        assert_eq!(error.code, "invalid_input");
    }

    #[tokio::test]
    async fn close_cancels_an_inflight_operation_before_releasing_state() {
        let directory = tempfile::tempdir().unwrap();
        let client = open(options(directory.path()), None).await.unwrap();
        let operation = client.inner.operation().await.unwrap();
        let inner = client.inner.clone();
        let close = tokio::spawn(async move { inner.close().await });
        tokio::task::yield_now().await;
        assert!(!close.is_finished());
        drop(operation);
        close.await.unwrap().unwrap();
        open(options(directory.path()), None).await.unwrap();
    }

    #[tokio::test]
    async fn close_error_still_reaches_closed_notifies_waiters_and_releases_state_lock() {
        let directory = tempfile::tempdir().unwrap();
        let client = open(options(directory.path()), None).await.unwrap();
        let expected = SafeError::new(
            "realtime_join_failed",
            "The realtime session failed while closing.",
            false,
        );
        *client
            .inner
            .realtime_close_error
            .lock()
            .expect("realtime close error lock") = Some(expected.clone());

        let error = client.inner.close().await.unwrap_err();

        assert_eq!(error, expected);
        tokio::time::timeout(Duration::from_millis(250), client.inner.close())
            .await
            .expect("a second close must not wait in closing")
            .unwrap();
        open(options(directory.path()), None).await.unwrap();
    }

    #[tokio::test]
    async fn clear_local_data_removes_owned_state_and_keeps_the_client_open() {
        let directory = tempfile::tempdir().unwrap();
        let client = open(options(directory.path()), None).await.unwrap();
        std::fs::write(directory.path().join("cache/owned.bin"), b"private").unwrap();
        std::fs::write(directory.path().join("vault/owned.bin"), b"private").unwrap();
        std::fs::write(directory.path().join("compatibility.json"), b"{}").unwrap();

        assert!(client.clear_local_data_inner().await.unwrap().cleared);
        assert!(!directory.path().join("cache/owned.bin").exists());
        assert!(!directory.path().join("vault/owned.bin").exists());
        assert!(!directory.path().join("compatibility.json").exists());
        assert_eq!(client.get_default_identity_inner().await.unwrap(), None);
        assert!(client.clear_local_data_inner().await.unwrap().cleared);
        assert_eq!(
            open(options(directory.path()), None)
                .await
                .err()
                .unwrap()
                .code,
            "state_in_use"
        );
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

    #[test]
    fn node_sync_defaults_use_core_supported_reasons() {
        let (request, _) = sync_request(None, Duration::from_secs(1)).unwrap();
        assert_eq!(request.reason, "manual_refresh");
        assert_eq!(LIST_CONVERSATIONS_SYNC_REASON, "foreground_reconcile");
    }

    #[test]
    fn node_transport_enables_core_owned_realtime_without_exposing_transport_details() {
        let directory = tempfile::tempdir().unwrap();
        let config = core_config(&options(directory.path())).unwrap();
        assert_eq!(
            config.transport_policy,
            im_core::MessageTransportPolicy::RealtimePreferred
        );
    }

    #[test]
    fn realtime_adapter_emits_sync_for_ready_reconnect_and_dirty_gap_hints() {
        let connected = im_core::realtime::ImEvent::ConnectionStateChanged(
            im_core::realtime::ConnectionStateChanged {
                state: im_core::realtime::RealtimeConnectionState::Connected,
                reason: Some("must-not-cross-the-node-boundary".to_owned()),
            },
        );
        let first = map_realtime_event(connected.clone(), false);
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].kind, "connection_state_changed");
        assert_eq!(first[0].state.as_deref(), Some("connected"));
        assert_eq!(first[1].cause.as_deref(), Some("connection_ready"));
        assert!(format!("{first:?}").contains("connection_ready"));
        assert!(!format!("{first:?}").contains("must-not-cross"));

        let reconnect = map_realtime_event(connected, true);
        assert_eq!(reconnect[1].cause.as_deref(), Some("reconnected"));

        let hint = im_core::realtime::RealtimeSyncHint {
            event_id: Some("raw-event-id".to_owned()),
            event_seq: Some("18446744073709551615".to_owned()),
            event_type: Some("raw.event.type".to_owned()),
            domains: Default::default(),
            reason: Some("raw recovery reason".to_owned()),
            dirty_lanes: Default::default(),
            sync_dirty: true,
            gap_detected: true,
            has_unknown_domain: false,
        };
        let required = sync_required_from_hint("message", Some(hint));
        assert_eq!(required.kind, "sync_required");
        assert_eq!(required.dirty, Some(true));
        assert_eq!(required.gap_detected, Some(true));
        let debug = format!("{required:?}");
        assert!(!debug.contains("raw-event-id"));
        assert!(!debug.contains("18446744073709551615"));
        assert!(!debug.contains("raw.event.type"));
        assert!(!debug.contains("raw recovery reason"));
    }

    #[test]
    fn realtime_options_default_to_bounded_exponential_reconnect() {
        let options = realtime_options(None).unwrap();
        assert_eq!(options.event_buffer, 128);
        assert_eq!(
            options.subscriptions,
            vec![im_core::realtime::RealtimeSubscription::Messages]
        );
        assert_eq!(
            options.reconnect,
            im_core::realtime::ReconnectPolicy::Exponential {
                base_delay_ms: 1_000,
                max_delay_ms: 30_000,
                max_attempts: None,
            }
        );
        assert!(realtime_options(Some(NodeRealtimeOptions {
            event_buffer: Some(MAX_REALTIME_EVENT_BUFFER + 1),
            reconnect_base_delay_ms: None,
            reconnect_max_delay_ms: None,
            reconnect_max_attempts: None,
        }))
        .is_err());
    }

    #[test]
    fn vault_migration_must_preserve_identity_id_did_and_handle() {
        let before = im_core::identity::IdentitySummary {
            id: im_core::ids::IdentityId::parse("alice").unwrap(),
            did: im_core::ids::Did::parse("did:example:alice").unwrap(),
            handle: Some(im_core::ids::Handle::parse("alice@example.test", "").unwrap()),
            display_name: Some("Before".to_owned()),
            local_alias: Some("default".to_owned()),
            device_id: None,
            is_default: true,
            readiness: im_core::identity::IdentityReadiness {
                ready_for_auth: true,
                ready_for_messaging: true,
                missing: Vec::new(),
            },
        };
        let mut after = before.clone();
        after.display_name = Some("After".to_owned());
        assert!(ensure_identity_preserved(&before, &after, true).is_ok());

        after.did = im_core::ids::Did::parse("did:example:replacement").unwrap();
        let error = ensure_identity_preserved(&before, &after, true).unwrap_err();
        assert_eq!(error.code, "identity_migration_failed");
        assert!(!error.safe_message.contains("did:example"));
        assert!(ensure_identity_preserved(&before, &before, false).is_err());
    }

    #[test]
    fn heavyweight_core_futures_use_a_boxed_dispatch_boundary() {
        let future: BoxImFuture<'static, ()> = box_im_future(async { Ok(()) });
        assert_eq!(
            std::mem::size_of_val(&future),
            2 * std::mem::size_of::<usize>()
        );
    }
}
