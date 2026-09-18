//! Join-bound management provisioning. Durable authority lives with the Join approval;
//! encrypted delivery remains owned by the existing P5 sender ledger.
use serde::{Deserialize, Serialize};

pub(crate) const MAX_ATTEMPTS: u8 = 4;
pub(crate) const RETRY_DELAY_MS: i64 = 5_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ManagementTask {
    pub phase: ManagementPhase,
    pub attempts: u8,
    #[serde(default = "legacy_max_attempts")]
    pub max_attempts: u8,
    pub next_attempt_at_ms: i64,
    pub failure_code: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagementPhase {
    AwaitingJoin,
    Scheduled,
    Attempting,
    WaitingForRecipient,
    ManagementRegistered,
    Failed,
}

fn legacy_max_attempts() -> u8 {
    3
}

impl ManagementTask {
    pub(crate) fn authorized() -> Self {
        Self {
            phase: ManagementPhase::AwaitingJoin,
            attempts: 0,
            max_attempts: MAX_ATTEMPTS,
            next_attempt_at_ms: 0,
            failure_code: None,
        }
    }

    pub(crate) fn activate(&mut self) {
        if self.phase == ManagementPhase::AwaitingJoin {
            self.phase = ManagementPhase::Scheduled;
        }
    }

    /// Called only with the cross-process worker lock held; the abandoned attempt
    /// remains charged. Reconciliation of the sender ledger precedes this call.
    pub(crate) fn recover_interrupted(&mut self, now_ms: i64) {
        if self.phase == ManagementPhase::Attempting {
            self.failed_attempt(now_ms, "interrupted", true);
        }
    }

    pub(crate) fn claim(&mut self, now_ms: i64) -> bool {
        if self.phase != ManagementPhase::Scheduled
            || self.attempts >= self.max_attempts
            || now_ms < self.next_attempt_at_ms
        {
            return false;
        }
        self.attempts += 1;
        self.phase = ManagementPhase::Attempting;
        self.failure_code = None;
        true
    }

    pub(crate) fn failed_attempt(&mut self, now_ms: i64, code: &str, retryable: bool) {
        self.failure_code = Some(code.to_owned());
        self.phase = if retryable && self.attempts < self.max_attempts {
            ManagementPhase::Scheduled
        } else {
            ManagementPhase::Failed
        };
        self.next_attempt_at_ms = now_ms.saturating_add(RETRY_DELAY_MS);
    }

    pub(crate) fn accepted(&mut self) {
        self.phase = ManagementPhase::WaitingForRecipient;
        self.failure_code = None;
    }
}

// This seam lets regression tests observe the production transition order with
// a controlled clock, durable-write failure and transport faults.
#[async_trait::async_trait]
trait TaskIo: Send {
    fn now_ms(&self) -> i64;
    fn accepted_locally(&mut self) -> crate::ImResult<bool>;
    fn delivery_expired(&mut self) -> crate::ImResult<bool>;
    fn persist(&mut self, task: &ManagementTask) -> crate::ImResult<()>;
    async fn registered(&mut self) -> crate::identity::RootKeyTransferResult<bool>;
    async fn send(&mut self) -> crate::identity::RootKeyTransferResult<()>;
}

fn retryable(code: crate::identity::RootKeyTransferErrorCode) -> bool {
    use crate::identity::RootKeyTransferErrorCode::*;
    matches!(
        code,
        PrekeyUnavailable
            | RootVaultUnavailable
            | AuthorizationExpired
            | TransportPending
            | TemporarilyUnavailable
    )
}

async fn advance_task(task: &mut ManagementTask, io: &mut impl TaskIo) -> crate::ImResult<()> {
    task.activate();
    let invalidated = task.phase == ManagementPhase::Failed
        && task.failure_code.as_deref() == Some("root_transfer.delivery_invalidated");
    if io.accepted_locally()? && task.phase != ManagementPhase::ManagementRegistered && !invalidated
    {
        task.accepted();
    }
    if task.attempts > 0
        || matches!(
            task.phase,
            ManagementPhase::WaitingForRecipient | ManagementPhase::ManagementRegistered
        )
    {
        match io.registered().await {
            Ok(true) => {
                task.phase = ManagementPhase::ManagementRegistered;
                task.failure_code = None;
                return io.persist(task);
            }
            // Keep a proven invalidation across transient reads, stale reads,
            // and restarts. Only the authoritative success above can clear it.
            _ if invalidated => return io.persist(task),
            Ok(false) => {}
            Err(error) if !retryable(error.code) => {
                task.failed_attempt(io.now_ms(), &error.to_string(), false);
                return io.persist(task);
            }
            Err(_) => {}
        }
    }
    if io.delivery_expired()? {
        task.failed_attempt(io.now_ms(), "root_transfer.delivery_expired", false);
        return io.persist(task);
    }
    if matches!(
        task.phase,
        ManagementPhase::WaitingForRecipient | ManagementPhase::ManagementRegistered
    ) {
        return io.persist(task);
    }
    task.recover_interrupted(io.now_ms());
    if task.claim(io.now_ms()) {
        // A failed durable charge must prevent even preflight/root export.
        io.persist(task)?;
        match io.send().await {
            Ok(()) => task.accepted(),
            Err(error) => {
                task.failed_attempt(io.now_ms(), &error.to_string(), retryable(error.code))
            }
        }
    }
    io.persist(task)
}

async fn retry_task(task: &mut ManagementTask, io: &mut impl TaskIo) -> crate::ImResult<()> {
    // Failure to reconcile leaves the previous round untouched.
    let registered = match io.registered().await {
        Ok(value) => value,
        Err(error)
            if error.code == crate::identity::RootKeyTransferErrorCode::DeliveryInvalidated =>
        {
            task.failed_attempt(io.now_ms(), &error.to_string(), false);
            io.persist(task)?;
            return Err(crate::ImError::PermissionDenied);
        }
        Err(_) => return Err(crate::ImError::PermissionDenied),
    };
    if registered {
        task.phase = ManagementPhase::ManagementRegistered;
        task.failure_code = None;
    } else if task.failure_code.as_deref() == Some("root_transfer.delivery_invalidated") {
        return Err(crate::ImError::PermissionDenied);
    } else if io.delivery_expired()? {
        task.failed_attempt(io.now_ms(), "root_transfer.delivery_expired", false);
        io.persist(task)?;
        return Err(crate::ImError::PermissionDenied);
    } else if io.accepted_locally()? {
        task.accepted();
    } else if task.phase == ManagementPhase::Failed {
        let max_attempts = task.max_attempts;
        *task = ManagementTask::authorized();
        task.max_attempts = max_attempts;
        task.activate();
    } else {
        return Err(crate::ImError::PermissionDenied);
    }
    io.persist(task)
}

#[cfg(test)]
#[path = "identity_join_management_tests.rs"]
mod tests;

#[cfg(feature = "sqlite")]
mod runtime {
    use super::*;
    use crate::internal::identity_device_join::management::{save_task, tasks};
    use crate::internal::identity_root_transfer_runtime as transfer;
    use fs2::FileExt;
    use sha2::{Digest, Sha256};

    fn now_ms() -> i64 {
        (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
    }

    // File ownership lasts across awaits and is released by the OS on process
    // death. No expiring lease can let another worker overlap a live sender.
    pub(super) fn worker_lock(
        client: &crate::core::ImClient,
    ) -> crate::ImResult<Option<std::fs::File>> {
        let key = format!(
            "{}:{}",
            client.current_identity().id.as_str(),
            client.exact_protocol_device_id()?.as_str()
        );
        let name = format!(".join-management-{:x}.lock", Sha256::digest(key.as_bytes()));
        let path = client
            .core_inner()
            .sdk_paths()
            .identities
            .identity_root_dir
            .join(name);
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path)?;
        match FileExt::try_lock_exclusive(&file) {
            Ok(()) => Ok(Some(file)),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn wait_for_worker_lock(
        client: &crate::core::ImClient,
    ) -> crate::ImResult<std::fs::File> {
        loop {
            if let Some(lock) = worker_lock(client)? {
                return Ok(lock);
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    }

    struct ProductionIo<'a> {
        client: &'a crate::core::ImClient,
        join: crate::internal::identity_device_join::management::AuthorizedManagementJoin,
    }

    #[async_trait::async_trait]
    impl TaskIo for ProductionIo<'_> {
        fn now_ms(&self) -> i64 {
            now_ms()
        }
        fn accepted_locally(&mut self) -> crate::ImResult<bool> {
            transfer::join_delivery_accepted(self.client, &self.join.recipient_device_id)
        }
        fn delivery_expired(&mut self) -> crate::ImResult<bool> {
            transfer::join_delivery_expired(self.client, &self.join.recipient_device_id)
        }
        fn persist(&mut self, task: &ManagementTask) -> crate::ImResult<()> {
            self.join.task = task.clone();
            save_task(&self.client.core_handle(), self.client, &self.join)
        }
        async fn registered(&mut self) -> crate::identity::RootKeyTransferResult<bool> {
            tokio::time::timeout(
                crate::internal::http::RESPONSE_TIMEOUT,
                transfer::join_management_registered(self.client, &self.join),
            )
            .await
            .unwrap_or_else(|_| {
                Err(crate::identity::RootKeyTransferError::new(
                    crate::identity::RootKeyTransferErrorCode::TemporarilyUnavailable,
                ))
            })
        }
        async fn send(&mut self) -> crate::identity::RootKeyTransferResult<()> {
            // Each HTTP request retains the project's 30s response / 10s connect
            // timeout. The whole attempt is separately bounded, not by retry delay.
            tokio::time::timeout(
                std::time::Duration::from_secs(60),
                transfer::send_for_authorized_join(self.client, &self.join),
            )
            .await
            .unwrap_or_else(|_| {
                Err(crate::identity::RootKeyTransferError::new(
                    crate::identity::RootKeyTransferErrorCode::TemporarilyUnavailable,
                ))
            })
            .map(|_| ())
        }
    }

    pub(crate) async fn run(client: &crate::core::ImClient) -> crate::ImResult<()> {
        run_with_wait(client, true).await
    }

    async fn run_with_wait(
        client: &crate::core::ImClient,
        wait_for_worker: bool,
    ) -> crate::ImResult<()> {
        // Avoid filesystem writes on clients without an opted-in approval.
        if tasks(&client.core_handle(), client)?.is_empty() {
            return Ok(());
        }
        let lock = if wait_for_worker {
            Some(wait_for_worker_lock(client).await?)
        } else {
            worker_lock(client)?
        };
        let Some(_lock) = lock else {
            return Ok(());
        };
        loop {
            let mut next = None;
            for mut join in tasks(&client.core_handle(), client)? {
                if join.task.phase == ManagementPhase::ManagementRegistered {
                    continue;
                }
                if !join.join_authorized {
                    if join.task.phase == ManagementPhase::Failed {
                        save_task(&client.core_handle(), client, &join)?;
                    }
                    continue;
                }
                let mut io = ProductionIo {
                    client,
                    join: join.clone(),
                };
                advance_task(&mut join.task, &mut io).await?;
                if join.task.phase == ManagementPhase::Scheduled {
                    next = Some(next.map_or(join.task.next_attempt_at_ms, |n: i64| {
                        n.min(join.task.next_attempt_at_ms)
                    }));
                }
            }
            let Some(next) = next else {
                return Ok(());
            };
            tokio::time::sleep(std::time::Duration::from_millis(
                next.saturating_sub(now_ms()).max(0) as u64,
            ))
            .await;
        }
    }

    pub(crate) async fn retry(
        client: &crate::core::ImClient,
        session: &str,
    ) -> crate::ImResult<()> {
        let _lock = wait_for_worker_lock(client).await?;
        let mut join = tasks(&client.core_handle(), client)?
            .into_iter()
            .find(|j| j.join_session_id == session && j.join_authorized)
            .ok_or(crate::ImError::PermissionDenied)?;
        let mut io = ProductionIo {
            client,
            join: join.clone(),
        };
        retry_task(&mut join.task, &mut io).await
    }

    pub(crate) fn start_worker(client: &crate::core::ImClient) {
        let client = client.clone();
        tokio::spawn(async move {
            let _ = run_with_wait(&client, false).await;
        });
    }
}

#[cfg(feature = "sqlite")]
pub(crate) use runtime::{retry, run, start_worker};

#[cfg(all(test, feature = "sqlite"))]
pub(crate) fn test_hold_worker_lock(
    client: &crate::core::ImClient,
) -> crate::ImResult<Option<std::fs::File>> {
    runtime::worker_lock(client)
}
