use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use im_core::messages::SyncDeltaRequest;
use im_core::realtime::{
    ImEvent, RealtimeConnectionState, RealtimeEventStream, RealtimeExitReason, RealtimeOptions,
    RealtimeSession, RealtimeStatus, RealtimeSyncHint,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use super::sanitize_error_message;
use crate::agent::{AgentDefinition, AgentIdentityRecord};
use crate::{DaemonConfig, DaemonState, ImCoreAdapter};

const REALTIME_EVENT_CHANNEL_CAPACITY: usize = 256;
const SESSION_JOIN_TIMEOUT: Duration = Duration::from_secs(5);
const RELIABLE_SYNC_DEBOUNCE: Duration = Duration::from_millis(250);
const RELIABLE_SYNC_RETRY_BASE: Duration = Duration::from_secs(5);
const RELIABLE_SYNC_RETRY_MAX: Duration = Duration::from_secs(60);
pub(super) const DEGRADED_POLL_FALLBACK_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RealtimeEndpointKind {
    MessageService,
}

impl RealtimeEndpointKind {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::MessageService => "message_service",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RealtimeSource {
    pub(super) agent_did: String,
    pub(super) endpoint_kind: RealtimeEndpointKind,
    pub(super) session_id: String,
    pub(super) generation: u64,
}

impl RealtimeSource {
    pub(super) fn detail_json(&self) -> serde_json::Value {
        json!({
            "endpoint_kind": self.endpoint_kind.as_str(),
            "session_id": self.session_id,
            "generation": self.generation,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DaemonRealtimeEvent {
    pub(super) source: RealtimeSource,
    pub(super) event: ImEvent,
    pub(super) channel_pressure: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RuntimeRealtimeNotification {
    Event(DaemonRealtimeEvent),
    SessionStatus {
        source: RealtimeSource,
        status: RealtimeStatus,
    },
    SessionEnded {
        source: RealtimeSource,
        reason: RealtimeExitReason,
        warnings: Vec<String>,
    },
}

struct RuntimeRealtimeAgentSession {
    fingerprint: String,
    generation: u64,
    client: im_core::ImClient,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

pub(super) struct RuntimeRealtimeSupervisor {
    config: DaemonConfig,
    state: DaemonState,
    im_core: ImCoreAdapter,
    sender: mpsc::Sender<RuntimeRealtimeNotification>,
    receiver: mpsc::Receiver<RuntimeRealtimeNotification>,
    sessions: HashMap<String, RuntimeRealtimeAgentSession>,
    next_generation: u64,
}

impl RuntimeRealtimeSupervisor {
    pub(super) async fn start(
        config: DaemonConfig,
        state: DaemonState,
        im_core: ImCoreAdapter,
    ) -> Result<Self> {
        let (sender, receiver) = mpsc::channel(REALTIME_EVENT_CHANNEL_CAPACITY);
        let mut supervisor = Self {
            config,
            state,
            im_core,
            sender,
            receiver,
            sessions: HashMap::new(),
            next_generation: 1,
        };
        supervisor.reconcile_active_agents().await?;
        Ok(supervisor)
    }

    pub(super) async fn recv(&mut self) -> Option<RuntimeRealtimeNotification> {
        self.receiver.recv().await
    }

    pub(super) fn client_for_source(&self, source: &RealtimeSource) -> Option<im_core::ImClient> {
        self.sessions
            .get(&source.agent_did)
            .filter(|session| session.generation == source.generation)
            .map(|session| session.client.clone())
    }

    pub(super) async fn remove_ended_session(&mut self, source: &RealtimeSource) {
        let should_remove = self
            .sessions
            .get(&source.agent_did)
            .map(|session| session.generation == source.generation)
            .unwrap_or(false);
        if should_remove {
            let _ = self.sessions.remove(&source.agent_did);
        }
    }

    pub(super) async fn reconcile_active_agents(&mut self) -> Result<()> {
        let agents = self.state.list_agent_definitions()?;
        let active_agent_dids = agents
            .iter()
            .map(|agent| agent.agent_did.clone())
            .collect::<HashSet<_>>();
        let stale_agent_dids = self
            .sessions
            .keys()
            .filter(|agent_did| !active_agent_dids.contains(*agent_did))
            .cloned()
            .collect::<Vec<_>>();
        for agent_did in stale_agent_dids {
            self.stop_agent_session(&agent_did).await;
        }

        for agent in agents {
            let snapshot = match self.load_agent_snapshot(&agent) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    record_realtime_supervisor_error(
                        &self.state,
                        "daemon.realtime.agent_snapshot.failed",
                        &agent.agent_did,
                        &error,
                    );
                    continue;
                }
            };
            let should_start = self
                .sessions
                .get(&snapshot.agent.agent_did)
                .map(|session| session.fingerprint != snapshot.fingerprint)
                .unwrap_or(true);
            if !should_start {
                continue;
            }
            if self.sessions.contains_key(&snapshot.agent.agent_did) {
                self.stop_agent_session(&snapshot.agent.agent_did).await;
            }
            if let Err(error) = self.start_agent_session(snapshot).await {
                record_realtime_supervisor_error(
                    &self.state,
                    "daemon.realtime.session.start.failed",
                    &agent.agent_did,
                    &error,
                );
            }
        }
        Ok(())
    }

    pub(super) async fn stop(mut self) {
        let agent_dids = self.sessions.keys().cloned().collect::<Vec<_>>();
        for agent_did in agent_dids {
            self.stop_agent_session(&agent_did).await;
        }
    }

    fn load_agent_snapshot(&self, agent: &AgentDefinition) -> Result<RealtimeAgentSnapshot> {
        let identity = self
            .state
            .load_agent_identity(&agent.agent_did)
            .with_context(|| format!("load realtime identity for agent {}", agent.agent_did))?;
        let jwt_token = self
            .state
            .load_agent_auth_token(&agent.agent_did)
            .with_context(|| format!("load realtime auth token for agent {}", agent.agent_did))?;
        let fingerprint =
            realtime_agent_fingerprint(&self.config, agent, &identity, jwt_token.as_deref())?;
        Ok(RealtimeAgentSnapshot {
            agent: agent.clone(),
            identity,
            jwt_token,
            fingerprint,
        })
    }

    async fn start_agent_session(&mut self, snapshot: RealtimeAgentSnapshot) -> Result<()> {
        let client = self.im_core.client_for_agent_identity(
            &self.config,
            &snapshot.identity,
            snapshot.jwt_token.as_deref(),
        )?;
        let mut session = client
            .realtime()
            .start_async(RealtimeOptions::default())
            .await
            .with_context(|| {
                format!(
                    "start realtime session for agent {}",
                    snapshot.agent.agent_did
                )
            })?;
        let events = session.subscribe().with_context(|| {
            format!(
                "attach realtime event stream for agent {}",
                snapshot.agent.agent_did
            )
        })?;
        let status = session.status_updates();
        let generation = self.next_generation;
        self.next_generation = self.next_generation.saturating_add(1);
        let source = RealtimeSource {
            agent_did: snapshot.agent.agent_did.clone(),
            endpoint_kind: RealtimeEndpointKind::MessageService,
            session_id: format!(
                "rt-{generation}-{}",
                stable_source_suffix(&snapshot.agent.agent_did)
            ),
            generation,
        };
        let (stop, stop_rx) = watch::channel(false);
        let task = spawn_realtime_reader(
            source,
            client.clone(),
            session,
            events,
            status,
            stop_rx,
            self.sender.clone(),
        );
        self.sessions.insert(
            snapshot.agent.agent_did,
            RuntimeRealtimeAgentSession {
                fingerprint: snapshot.fingerprint,
                generation,
                client,
                stop,
                task,
            },
        );
        Ok(())
    }

    async fn stop_agent_session(&mut self, agent_did: &str) {
        let Some(session) = self.sessions.remove(agent_did) else {
            return;
        };
        let _ = session.stop.send(true);
        let abort_handle = session.task.abort_handle();
        if tokio::time::timeout(SESSION_JOIN_TIMEOUT, session.task)
            .await
            .is_err()
        {
            abort_handle.abort();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RealtimeDirtyReason {
    SyncHint,
    GapDetected,
    TargetedContext,
    Reconnected,
    Disconnected,
    UnknownNotification,
    SessionEnded,
    ChannelPressure,
}

impl RealtimeDirtyReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::SyncHint => "sync_hint",
            Self::GapDetected => "gap_detected",
            Self::TargetedContext => "targeted_context",
            Self::Reconnected => "reconnected",
            Self::Disconnected => "disconnected",
            Self::UnknownNotification => "unknown_notification",
            Self::SessionEnded => "session_ended",
            Self::ChannelPressure => "channel_pressure",
        }
    }

    fn requires_degraded_poll(self) -> bool {
        matches!(
            self,
            Self::Disconnected
                | Self::UnknownNotification
                | Self::SessionEnded
                | Self::ChannelPressure
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RuntimeRealtimeSyncWork {
    pub(super) agent_did: String,
    pub(super) source: Option<RealtimeSource>,
    pub(super) reasons: Vec<&'static str>,
    pub(super) degraded_poll: bool,
    pub(super) threads: Vec<im_core::messages::ThreadRef>,
    pub(super) groups: Vec<im_core::ids::GroupRef>,
}

#[derive(Debug, Clone)]
struct DirtyAgentSync {
    source: Option<RealtimeSource>,
    reasons: HashSet<&'static str>,
    threads: HashMap<String, im_core::messages::ThreadRef>,
    groups: HashSet<im_core::ids::GroupRef>,
    due_at: Instant,
    attempt_count: u32,
    degraded_poll: bool,
    snapshot_required: bool,
}

impl DirtyAgentSync {
    fn new(now: Instant, source: Option<RealtimeSource>) -> Self {
        Self {
            source,
            reasons: HashSet::new(),
            threads: HashMap::new(),
            groups: HashSet::new(),
            due_at: now + RELIABLE_SYNC_DEBOUNCE,
            attempt_count: 0,
            degraded_poll: false,
            snapshot_required: false,
        }
    }
}

pub(super) struct RuntimeRealtimeSyncCoordinator {
    dirty: HashMap<String, DirtyAgentSync>,
}

impl RuntimeRealtimeSyncCoordinator {
    pub(super) fn new() -> Self {
        Self {
            dirty: HashMap::new(),
        }
    }

    pub(super) fn mark_sync_hint(
        &mut self,
        source: &RealtimeSource,
        hint: Option<&RealtimeSyncHint>,
        thread: Option<im_core::messages::ThreadRef>,
        group: Option<im_core::ids::GroupRef>,
    ) {
        let Some(hint) = hint else {
            return;
        };
        if !hint.sync_dirty && !hint.gap_detected {
            return;
        }
        self.mark_dirty(
            source,
            RealtimeDirtyReason::SyncHint,
            thread.clone(),
            group.clone(),
        );
        if hint.gap_detected {
            self.mark_dirty(source, RealtimeDirtyReason::GapDetected, thread, group);
        }
    }

    pub(super) fn mark_targeted_context(
        &mut self,
        source: &RealtimeSource,
        thread: im_core::messages::ThreadRef,
        group: Option<im_core::ids::GroupRef>,
    ) {
        self.mark_dirty(
            source,
            RealtimeDirtyReason::TargetedContext,
            Some(thread),
            group,
        );
    }

    pub(super) fn mark_connection_state(
        &mut self,
        source: &RealtimeSource,
        state: RealtimeConnectionState,
    ) {
        match state {
            RealtimeConnectionState::Connected => {
                self.mark_dirty(source, RealtimeDirtyReason::Reconnected, None, None);
            }
            RealtimeConnectionState::Disconnected | RealtimeConnectionState::Closed => {
                self.mark_dirty(source, RealtimeDirtyReason::Disconnected, None, None);
            }
            RealtimeConnectionState::Connecting | RealtimeConnectionState::Reconnecting => {}
        }
    }

    pub(super) fn mark_unknown_notification(
        &mut self,
        source: &RealtimeSource,
        hint: Option<&RealtimeSyncHint>,
    ) {
        self.mark_dirty(source, RealtimeDirtyReason::UnknownNotification, None, None);
        self.mark_sync_hint(source, hint, None, None);
    }

    pub(super) fn mark_session_ended(&mut self, source: &RealtimeSource) {
        self.mark_dirty(source, RealtimeDirtyReason::SessionEnded, None, None);
    }

    pub(super) fn mark_channel_pressure(&mut self, source: &RealtimeSource) {
        self.mark_dirty(source, RealtimeDirtyReason::ChannelPressure, None, None);
    }

    fn mark_dirty(
        &mut self,
        source: &RealtimeSource,
        reason: RealtimeDirtyReason,
        thread: Option<im_core::messages::ThreadRef>,
        group: Option<im_core::ids::GroupRef>,
    ) {
        let now = Instant::now();
        let entry = self
            .dirty
            .entry(source.agent_did.clone())
            .or_insert_with(|| DirtyAgentSync::new(now, Some(source.clone())));
        if entry.snapshot_required {
            return;
        }
        entry.source = Some(source.clone());
        let was_empty = entry.reasons.is_empty();
        entry.reasons.insert(reason.as_str());
        if let Some(thread) = thread {
            entry.threads.insert(thread_ref_key(&thread), thread);
        }
        if let Some(group) = group {
            entry.groups.insert(group);
        }
        if reason.requires_degraded_poll() {
            entry.degraded_poll = true;
        }
        let retry_delay = sync_retry_delay_with_jitter(&source.agent_did, entry.attempt_count);
        let next_due = now + RELIABLE_SYNC_DEBOUNCE.max(retry_delay);
        if next_due < entry.due_at || was_empty {
            entry.due_at = next_due;
        }
    }

    pub(super) fn next_due_delay(&self) -> Duration {
        let now = Instant::now();
        self.dirty
            .values()
            .filter(|entry| !entry.snapshot_required)
            .map(|entry| entry.due_at.saturating_duration_since(now))
            .min()
            .unwrap_or(DEGRADED_POLL_FALLBACK_INTERVAL)
            .min(DEGRADED_POLL_FALLBACK_INTERVAL)
    }

    pub(super) fn take_due_work(&mut self) -> Vec<RuntimeRealtimeSyncWork> {
        let now = Instant::now();
        let due_agent_dids = self
            .dirty
            .iter()
            .filter(|(_, entry)| !entry.snapshot_required && entry.due_at <= now)
            .map(|(agent_did, _)| agent_did.clone())
            .collect::<Vec<_>>();
        due_agent_dids
            .into_iter()
            .filter_map(|agent_did| {
                self.dirty
                    .remove(&agent_did)
                    .map(|entry| RuntimeRealtimeSyncWork {
                        agent_did,
                        source: entry.source,
                        reasons: sorted_reasons(entry.reasons),
                        degraded_poll: entry.degraded_poll,
                        threads: sorted_threads(entry.threads),
                        groups: entry.groups.into_iter().collect(),
                    })
            })
            .collect()
    }

    pub(super) fn mark_work_retry(&mut self, work: &RuntimeRealtimeSyncWork) {
        let now = Instant::now();
        let entry = self
            .dirty
            .entry(work.agent_did.clone())
            .or_insert_with(|| DirtyAgentSync::new(now, work.source.clone()));
        entry.source = work.source.clone().or_else(|| entry.source.clone());
        entry.attempt_count = entry.attempt_count.saturating_add(1);
        entry.due_at = now + sync_retry_delay_with_jitter(&work.agent_did, entry.attempt_count);
        entry.degraded_poll |= work.degraded_poll;
        for reason in &work.reasons {
            entry.reasons.insert(*reason);
        }
        for thread in &work.threads {
            entry.threads.insert(thread_ref_key(thread), thread.clone());
        }
        for group in &work.groups {
            entry.groups.insert(group.clone());
        }
    }

    pub(super) fn mark_snapshot_required(&mut self, work: &RuntimeRealtimeSyncWork) {
        let now = Instant::now();
        let entry = self
            .dirty
            .entry(work.agent_did.clone())
            .or_insert_with(|| DirtyAgentSync::new(now, work.source.clone()));
        entry.snapshot_required = true;
        entry.reasons.insert("snapshot_required");
        entry.degraded_poll = false;
    }

    pub(super) fn dirty_agent_count(&self) -> usize {
        self.dirty
            .values()
            .filter(|entry| !entry.snapshot_required)
            .count()
    }
}

pub(super) async fn run_realtime_sync_delta(
    client: &im_core::ImClient,
    work: &RuntimeRealtimeSyncWork,
) -> im_core::ImResult<im_core::messages::SyncDeltaResult> {
    client
        .messages()
        .sync_delta_async(SyncDeltaRequest {
            limit: Some(100),
            device_id: None,
            reason: Some(format!("daemon_realtime:{}", work.reasons.join(","))),
        })
        .await
}

fn sync_retry_delay(attempt_count: u32) -> Duration {
    if attempt_count == 0 {
        return Duration::from_millis(0);
    }
    let shift = attempt_count.saturating_sub(1).min(4);
    let multiplier = 1_u32.checked_shl(shift).unwrap_or(16);
    (RELIABLE_SYNC_RETRY_BASE * multiplier).min(RELIABLE_SYNC_RETRY_MAX)
}

fn sync_retry_delay_with_jitter(agent_did: &str, attempt_count: u32) -> Duration {
    let base = sync_retry_delay(attempt_count);
    if attempt_count == 0 || base >= RELIABLE_SYNC_RETRY_MAX {
        return base;
    }
    base + deterministic_retry_jitter(agent_did, attempt_count)
}

fn deterministic_retry_jitter(agent_did: &str, attempt_count: u32) -> Duration {
    let mut hasher = Sha256::new();
    hasher.update(agent_did.as_bytes());
    hasher.update(attempt_count.to_le_bytes());
    let digest = hasher.finalize();
    let jitter_ms = u16::from_le_bytes([digest[0], digest[1]]) % 1_000;
    Duration::from_millis(u64::from(jitter_ms))
}

fn sorted_reasons(reasons: HashSet<&'static str>) -> Vec<&'static str> {
    let mut reasons = reasons.into_iter().collect::<Vec<_>>();
    reasons.sort_unstable();
    reasons
}

fn sorted_threads(
    threads: HashMap<String, im_core::messages::ThreadRef>,
) -> Vec<im_core::messages::ThreadRef> {
    let mut threads = threads.into_iter().collect::<Vec<_>>();
    threads.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    threads.into_iter().map(|(_, thread)| thread).collect()
}

fn thread_ref_key(thread: &im_core::messages::ThreadRef) -> String {
    match thread {
        im_core::messages::ThreadRef::Direct(peer) => format!("direct:{}", peer.as_str()),
        im_core::messages::ThreadRef::Group(group) => format!("group:{}", group.as_str()),
        im_core::messages::ThreadRef::Thread(thread_id) => {
            format!("thread:{}", thread_id.as_str())
        }
    }
}

impl Drop for RuntimeRealtimeSupervisor {
    fn drop(&mut self) {
        for (_, session) in self.sessions.drain() {
            let _ = session.stop.send(true);
            session.task.abort();
        }
    }
}

struct RealtimeAgentSnapshot {
    agent: AgentDefinition,
    identity: AgentIdentityRecord,
    jwt_token: Option<String>,
    fingerprint: String,
}

fn spawn_realtime_reader(
    source: RealtimeSource,
    client: im_core::ImClient,
    session: RealtimeSession,
    events: RealtimeEventStream,
    status: watch::Receiver<RealtimeStatus>,
    stop: watch::Receiver<bool>,
    output: mpsc::Sender<RuntimeRealtimeNotification>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        run_realtime_reader(source, client, session, events, status, stop, output).await;
    })
}

async fn run_realtime_reader(
    source: RealtimeSource,
    _client: im_core::ImClient,
    mut session: RealtimeSession,
    events: RealtimeEventStream,
    status: watch::Receiver<RealtimeStatus>,
    stop: watch::Receiver<bool>,
    output: mpsc::Sender<RuntimeRealtimeNotification>,
) {
    let exit = run_realtime_fan_in_loop(source.clone(), events, status, stop, output.clone()).await;
    if matches!(
        exit,
        RealtimeReaderExit::StopRequested | RealtimeReaderExit::OutputClosed
    ) {
        let _ = session.stop().await;
    }
    let (reason, warnings) = match tokio::time::timeout(SESSION_JOIN_TIMEOUT, session.join()).await
    {
        Ok(Ok(exit)) => {
            if !exit.warnings.is_empty() {
                let warnings = exit
                    .warnings
                    .iter()
                    .map(|warning| sanitize_error_message(warning))
                    .collect::<Vec<_>>();
                let _ = output
                    .send(RuntimeRealtimeNotification::SessionEnded {
                        source,
                        reason: exit.reason.clone(),
                        warnings,
                    })
                    .await;
                return;
            }
            (exit.reason, Vec::new())
        }
        Ok(Err(error)) => RealtimeExitReason::FatalError.with_warning(error.to_string()),
        Err(_) => RealtimeExitReason::FatalError.with_warning("realtime session join timed out"),
    };
    let _ = output
        .send(RuntimeRealtimeNotification::SessionEnded {
            source,
            reason,
            warnings,
        })
        .await;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RealtimeReaderExit {
    StopRequested,
    EventStreamClosed,
    OutputClosed,
}

async fn run_realtime_fan_in_loop(
    source: RealtimeSource,
    mut events: RealtimeEventStream,
    mut status: watch::Receiver<RealtimeStatus>,
    mut stop: watch::Receiver<bool>,
    output: mpsc::Sender<RuntimeRealtimeNotification>,
) -> RealtimeReaderExit {
    loop {
        tokio::select! {
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() {
                    return RealtimeReaderExit::StopRequested;
                }
            }
            changed = status.changed() => {
                if changed.is_ok() {
                    let mut status = status.borrow().clone();
                    status.last_error = status
                        .last_error
                        .as_deref()
                        .map(sanitize_error_message);
                    if output
                        .send(RuntimeRealtimeNotification::SessionStatus {
                            source: source.clone(),
                            status,
                        })
                        .await
                        .is_err()
                    {
                        return RealtimeReaderExit::OutputClosed;
                    }
                }
            }
            event = events.recv() => {
                let Some(event) = event else {
                    return RealtimeReaderExit::EventStreamClosed;
                };
                let channel_pressure = output.capacity() == 0;
                if output
                    .send(RuntimeRealtimeNotification::Event(DaemonRealtimeEvent {
                        source: source.clone(),
                        event,
                        channel_pressure,
                    }))
                    .await
                    .is_err()
                {
                    return RealtimeReaderExit::OutputClosed;
                }
            }
        }
    }
}

trait RealtimeExitWarning {
    fn with_warning(self, warning: impl Into<String>) -> (Self, Vec<String>)
    where
        Self: Sized;
}

impl RealtimeExitWarning for RealtimeExitReason {
    fn with_warning(self, warning: impl Into<String>) -> (Self, Vec<String>) {
        (self, vec![sanitize_error_message(&warning.into())])
    }
}

fn realtime_agent_fingerprint(
    config: &DaemonConfig,
    agent: &AgentDefinition,
    identity: &AgentIdentityRecord,
    jwt_token: Option<&str>,
) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(agent.agent_did.as_bytes());
    hasher.update(agent.status.as_bytes());
    hasher.update(identity.handle.as_bytes());
    hasher.update(serde_json::to_vec(&identity.did_document)?);
    hasher.update(identity.public_key.as_bytes());
    hasher.update(identity.auth_private_key_pem.as_bytes());
    hasher.update(identity.e2ee_signing_private_key_pem.as_bytes());
    hasher.update(identity.e2ee_agreement_private_key_pem.as_bytes());
    if let Some(token) = jwt_token {
        hasher.update(token.as_bytes());
    }
    hasher.update(config.service_base_url.as_bytes());
    hasher.update(config.message_service_base_url.as_bytes());
    hasher.update(config.did_domain.as_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}

fn stable_source_suffix(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
        .chars()
        .take(12)
        .collect()
}

fn record_realtime_supervisor_error(
    state: &DaemonState,
    event_type: &str,
    agent_did: &str,
    error: &anyhow::Error,
) {
    let sanitized = sanitize_error_message(&error.to_string());
    eprintln!("warning: {event_type} for agent {agent_did}: {sanitized}");
    if let Err(audit_error) = state.insert_audit_event_json(
        event_type,
        Some(agent_did),
        None,
        None,
        None,
        json!({ "error": sanitized }),
    ) {
        eprintln!(
            "warning: daemon realtime error audit failed: {}",
            sanitize_error_message(&audit_error.to_string())
        );
    }
}

pub(super) fn realtime_status_detail(status: &RealtimeStatus) -> serde_json::Value {
    json!({
        "connected": status.connected,
        "state": realtime_connection_state_name(status.state.clone()),
        "subscriptions": status
            .subscriptions
            .iter()
            .map(|subscription| format!("{subscription:?}"))
            .collect::<Vec<_>>(),
        "last_error": status.last_error.as_deref().map(sanitize_error_message),
    })
}

pub(super) fn realtime_connection_state_name(state: RealtimeConnectionState) -> &'static str {
    match state {
        RealtimeConnectionState::Disconnected => "disconnected",
        RealtimeConnectionState::Connecting => "connecting",
        RealtimeConnectionState::Connected => "connected",
        RealtimeConnectionState::Reconnecting => "reconnecting",
        RealtimeConnectionState::Closed => "closed",
    }
}

pub(super) fn realtime_exit_reason_name(reason: &RealtimeExitReason) -> &'static str {
    match reason {
        RealtimeExitReason::ShutdownRequested => "shutdown_requested",
        RealtimeExitReason::ConnectionClosed => "connection_closed",
        RealtimeExitReason::AuthFailed => "auth_failed",
        RealtimeExitReason::TransportUnavailable => "transport_unavailable",
        RealtimeExitReason::FatalError => "fatal_error",
    }
}

#[cfg(test)]
mod tests {
    use im_core::ids::{MessageId, PeerRef};
    use im_core::messages::{
        Message, MessageBodyView, MessageDirection, MessageKind, MessageMetadata, ThreadRef,
    };
    use tokio::sync::{mpsc, watch};

    use super::*;

    fn source(agent_did: &str, generation: u64) -> RealtimeSource {
        RealtimeSource {
            agent_did: agent_did.to_string(),
            endpoint_kind: RealtimeEndpointKind::MessageService,
            session_id: format!("session-{generation}"),
            generation,
        }
    }

    fn direct_event(message_id: &str) -> ImEvent {
        ImEvent::MessageReceived(im_core::realtime::MessageReceivedEvent {
            message: Message {
                id: MessageId::parse(message_id).unwrap(),
                thread: ThreadRef::Direct(PeerRef::parse("did:human:alice", "").unwrap()),
                direction: MessageDirection::Incoming,
                sender: PeerRef::parse("did:human:alice", "").unwrap(),
                receiver: Some(PeerRef::parse("did:agent:runtime", "").unwrap()),
                group: None,
                body: MessageBodyView::Text {
                    text: "hello realtime".to_string(),
                    kind: MessageKind::Text,
                },
                sent_at: None,
                received_at: None,
                metadata: MessageMetadata::default(),
            },
            attachment_summary: None,
            download_action: None,
            sync: None,
            warnings: Vec::new(),
        })
    }

    fn sync_hint(gap_detected: bool) -> RealtimeSyncHint {
        RealtimeSyncHint {
            event_id: Some("evt-1".to_string()),
            event_seq: Some("42".to_string()),
            event_type: Some("message.created".to_string()),
            sync_dirty: true,
            gap_detected,
        }
    }

    fn status_receiver() -> watch::Receiver<RealtimeStatus> {
        let (_sender, receiver) = watch::channel(RealtimeStatus {
            connected: false,
            state: RealtimeConnectionState::Disconnected,
            subscriptions: RealtimeOptions::default().subscriptions,
            last_error: None,
        });
        receiver
    }

    #[test]
    fn sync_coordinator_coalesces_sync_hint_and_gap_by_agent() {
        let source = source("did:agent:a", 1);
        let group = im_core::ids::GroupRef::parse("did:group:team").unwrap();
        let mut coordinator = RuntimeRealtimeSyncCoordinator::new();

        coordinator.mark_sync_hint(&source, Some(&sync_hint(false)), None, None);
        coordinator.mark_sync_hint(
            &source,
            Some(&sync_hint(true)),
            Some(im_core::messages::ThreadRef::Group(group.clone())),
            Some(group.clone()),
        );

        std::thread::sleep(RELIABLE_SYNC_DEBOUNCE + Duration::from_millis(20));
        let work = coordinator.take_due_work();

        assert_eq!(work.len(), 1);
        assert_eq!(work[0].agent_did, "did:agent:a");
        assert_eq!(work[0].groups, vec![group]);
        assert_eq!(work[0].reasons, vec!["gap_detected", "sync_hint"]);
        assert!(!work[0].degraded_poll);
    }

    #[test]
    fn sync_coordinator_marks_unknown_notification_for_degraded_fallback() {
        let source = source("did:agent:a", 1);
        let mut coordinator = RuntimeRealtimeSyncCoordinator::new();

        coordinator.mark_unknown_notification(&source, Some(&sync_hint(false)));

        std::thread::sleep(RELIABLE_SYNC_DEBOUNCE + Duration::from_millis(20));
        let work = coordinator.take_due_work();

        assert_eq!(work.len(), 1);
        assert_eq!(work[0].reasons, vec!["sync_hint", "unknown_notification"]);
        assert!(work[0].degraded_poll);
    }

    #[test]
    fn sync_coordinator_marks_reconnect_without_degraded_poll() {
        let source = source("did:agent:a", 1);
        let mut coordinator = RuntimeRealtimeSyncCoordinator::new();

        coordinator.mark_connection_state(&source, RealtimeConnectionState::Connected);

        std::thread::sleep(RELIABLE_SYNC_DEBOUNCE + Duration::from_millis(20));
        let work = coordinator.take_due_work();

        assert_eq!(work.len(), 1);
        assert_eq!(work[0].reasons, vec!["reconnected"]);
        assert!(!work[0].degraded_poll);
    }

    #[test]
    fn sync_coordinator_keeps_original_due_for_repeated_same_reason() {
        let source = source("did:agent:a", 1);
        let mut coordinator = RuntimeRealtimeSyncCoordinator::new();

        coordinator.mark_sync_hint(&source, Some(&sync_hint(false)), None, None);
        std::thread::sleep(RELIABLE_SYNC_DEBOUNCE / 2);
        coordinator.mark_sync_hint(&source, Some(&sync_hint(false)), None, None);

        std::thread::sleep(RELIABLE_SYNC_DEBOUNCE / 2 + Duration::from_millis(20));
        let work = coordinator.take_due_work();

        assert_eq!(work.len(), 1);
        assert_eq!(work[0].reasons, vec!["sync_hint"]);
    }

    #[test]
    fn sync_coordinator_tracks_targeted_threads_and_groups() {
        let source = source("did:agent:a", 1);
        let group = im_core::ids::GroupRef::parse("did:group:team").unwrap();
        let direct = im_core::ids::PeerRef::parse("did:peer:alice", "").unwrap();
        let mut coordinator = RuntimeRealtimeSyncCoordinator::new();

        coordinator.mark_sync_hint(
            &source,
            Some(&sync_hint(false)),
            Some(im_core::messages::ThreadRef::Direct(direct.clone())),
            None,
        );
        coordinator.mark_sync_hint(
            &source,
            Some(&sync_hint(false)),
            Some(im_core::messages::ThreadRef::Group(group.clone())),
            Some(group.clone()),
        );

        std::thread::sleep(RELIABLE_SYNC_DEBOUNCE + Duration::from_millis(20));
        let work = coordinator.take_due_work();

        assert_eq!(work.len(), 1);
        assert_eq!(
            work[0].threads,
            vec![
                im_core::messages::ThreadRef::Direct(direct),
                im_core::messages::ThreadRef::Group(group.clone()),
            ]
        );
        assert_eq!(work[0].groups, vec![group]);
    }

    #[test]
    fn sync_coordinator_tracks_targeted_context_without_sync_hint() {
        let source = source("did:agent:a", 1);
        let group = im_core::ids::GroupRef::parse("did:group:team").unwrap();
        let mut coordinator = RuntimeRealtimeSyncCoordinator::new();

        coordinator.mark_targeted_context(
            &source,
            im_core::messages::ThreadRef::Group(group.clone()),
            Some(group.clone()),
        );

        std::thread::sleep(RELIABLE_SYNC_DEBOUNCE + Duration::from_millis(20));
        let work = coordinator.take_due_work();

        assert_eq!(work.len(), 1);
        assert_eq!(work[0].reasons, vec!["targeted_context"]);
        assert_eq!(
            work[0].threads,
            vec![im_core::messages::ThreadRef::Group(group.clone())]
        );
        assert_eq!(work[0].groups, vec![group]);
        assert!(!work[0].degraded_poll);
    }

    #[test]
    fn sync_coordinator_snapshot_required_blocks_future_due_work() {
        let source = source("did:agent:a", 1);
        let mut coordinator = RuntimeRealtimeSyncCoordinator::new();
        coordinator.mark_session_ended(&source);

        std::thread::sleep(RELIABLE_SYNC_DEBOUNCE + Duration::from_millis(20));
        let work = coordinator.take_due_work().pop().unwrap();
        coordinator.mark_snapshot_required(&work);
        coordinator.mark_sync_hint(&source, Some(&sync_hint(true)), None, None);

        assert!(coordinator.take_due_work().is_empty());
        assert_eq!(coordinator.dirty_agent_count(), 0);
    }

    #[test]
    fn sync_coordinator_retry_uses_backoff_and_keeps_reasons() {
        let source = source("did:agent:a", 1);
        let mut coordinator = RuntimeRealtimeSyncCoordinator::new();
        coordinator.mark_session_ended(&source);

        std::thread::sleep(RELIABLE_SYNC_DEBOUNCE + Duration::from_millis(20));
        let work = coordinator.take_due_work().pop().unwrap();
        coordinator.mark_work_retry(&work);

        assert!(coordinator.take_due_work().is_empty());
        assert!(coordinator.next_due_delay() >= Duration::from_secs(4));
        assert_eq!(coordinator.dirty_agent_count(), 1);
    }

    #[test]
    fn sync_retry_delay_uses_deterministic_jitter() {
        let retry_a = sync_retry_delay_with_jitter("did:agent:a", 1);
        let retry_a_again = sync_retry_delay_with_jitter("did:agent:a", 1);
        let retry_b = sync_retry_delay_with_jitter("did:agent:b", 1);

        assert_eq!(retry_a, retry_a_again);
        assert!(retry_a >= RELIABLE_SYNC_RETRY_BASE);
        assert!(retry_a < RELIABLE_SYNC_RETRY_BASE + Duration::from_secs(1));
        assert_ne!(retry_a, retry_b);
    }

    #[tokio::test]
    async fn realtime_fan_in_preserves_source_for_multiple_agents() {
        let (output_tx, mut output_rx) = mpsc::channel(8);
        let (stop_tx, stop_rx_a) = watch::channel(false);
        let stop_rx_b = stop_tx.subscribe();
        let (events_tx_a, events_rx_a) = mpsc::channel(8);
        let (events_tx_b, events_rx_b) = mpsc::channel(8);
        let task_a = tokio::spawn(run_realtime_fan_in_loop(
            source("did:agent:a", 1),
            events_rx_a,
            status_receiver(),
            stop_rx_a,
            output_tx.clone(),
        ));
        let task_b = tokio::spawn(run_realtime_fan_in_loop(
            source("did:agent:b", 2),
            events_rx_b,
            status_receiver(),
            stop_rx_b,
            output_tx,
        ));

        events_tx_a.send(direct_event("msg-a")).await.unwrap();
        events_tx_b.send(direct_event("msg-b")).await.unwrap();

        let first = output_rx.recv().await.unwrap();
        let second = output_rx.recv().await.unwrap();
        let mut seen = [first, second]
            .into_iter()
            .map(|notification| match notification {
                RuntimeRealtimeNotification::Event(event) => {
                    (event.source.agent_did, event.source.generation)
                }
                other => panic!("expected realtime event, got {other:?}"),
            })
            .collect::<Vec<_>>();
        seen.sort();
        assert_eq!(
            seen,
            vec![
                ("did:agent:a".to_string(), 1),
                ("did:agent:b".to_string(), 2)
            ]
        );

        let _ = stop_tx.send(true);
        assert_eq!(task_a.await.unwrap(), RealtimeReaderExit::StopRequested);
        assert_eq!(task_b.await.unwrap(), RealtimeReaderExit::StopRequested);
    }

    #[tokio::test]
    async fn realtime_fan_in_forwards_status_without_secret_detail() {
        let (output_tx, mut output_rx) = mpsc::channel(8);
        let (stop_tx, stop_rx) = watch::channel(false);
        let (_events_tx, events_rx) = mpsc::channel(8);
        let (status_tx, status_rx) = watch::channel(RealtimeStatus {
            connected: false,
            state: RealtimeConnectionState::Disconnected,
            subscriptions: RealtimeOptions::default().subscriptions,
            last_error: None,
        });
        let task = tokio::spawn(run_realtime_fan_in_loop(
            source("did:agent:a", 1),
            events_rx,
            status_rx,
            stop_rx,
            output_tx,
        ));

        status_tx
            .send(RealtimeStatus {
                connected: false,
                state: RealtimeConnectionState::Closed,
                subscriptions: RealtimeOptions::default().subscriptions,
                last_error: Some("jwt token private.key secret".to_string()),
            })
            .unwrap();
        let notification = output_rx.recv().await.unwrap();
        match notification {
            RuntimeRealtimeNotification::SessionStatus { status, .. } => {
                assert_eq!(
                    status.last_error.as_deref(),
                    Some("<redacted> <redacted> <redacted> <redacted>")
                );
            }
            other => panic!("expected status notification, got {other:?}"),
        }

        let _ = stop_tx.send(true);
        assert_eq!(task.await.unwrap(), RealtimeReaderExit::StopRequested);
    }

    #[tokio::test]
    async fn realtime_fan_in_waits_when_output_channel_is_full() {
        let (output_tx, mut output_rx) = mpsc::channel(1);
        let (stop_tx, stop_rx) = watch::channel(false);
        let (events_tx, events_rx) = mpsc::channel(8);
        let task = tokio::spawn(run_realtime_fan_in_loop(
            source("did:agent:a", 1),
            events_rx,
            status_receiver(),
            stop_rx,
            output_tx,
        ));

        events_tx.send(direct_event("msg-a")).await.unwrap();
        events_tx.send(direct_event("msg-b")).await.unwrap();

        let first = output_rx.recv().await.unwrap();
        match first {
            RuntimeRealtimeNotification::Event(event) => {
                assert!(!event.channel_pressure);
                assert_eq!(event.source.agent_did, "did:agent:a");
                assert_eq!(event.source.generation, 1);
            }
            other => panic!("expected realtime event, got {other:?}"),
        }
        let second = output_rx.recv().await.unwrap();
        match second {
            RuntimeRealtimeNotification::Event(event) => {
                assert!(event.channel_pressure);
                let im_core::realtime::ImEvent::MessageReceived(message) = event.event else {
                    panic!("expected message event");
                };
                assert_eq!(message.message.id.as_str(), "msg-b");
            }
            other => panic!("expected realtime event, got {other:?}"),
        }

        let _ = stop_tx.send(true);
        assert_eq!(task.await.unwrap(), RealtimeReaderExit::StopRequested);
    }

    #[tokio::test]
    async fn realtime_fan_in_reports_event_stream_closed() {
        let (output_tx, _output_rx) = mpsc::channel(8);
        let (_stop_tx, stop_rx) = watch::channel(false);
        let (events_tx, events_rx) = mpsc::channel(8);
        drop(events_tx);

        let exit = run_realtime_fan_in_loop(
            source("did:agent:a", 1),
            events_rx,
            status_receiver(),
            stop_rx,
            output_tx,
        )
        .await;

        assert_eq!(exit, RealtimeReaderExit::EventStreamClosed);
    }
}
