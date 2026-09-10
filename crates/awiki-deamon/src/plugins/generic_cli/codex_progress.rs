use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use serde_json::{json, Value};

use crate::cli_wrapper::{self, CliWrapperRequest};
use crate::commands::latest_value_dispatcher::LatestValueDispatcher;
use crate::runtime::{
    RuntimeProgressCode, RuntimeProgressPhase, RuntimeProgressState, RuntimeProgressUpdate,
};

pub(super) struct CodexProgressReporter {
    dispatcher: Option<LatestValueDispatcher<bool>>,
    delivered: Arc<AtomicUsize>,
    failed: Arc<AtomicUsize>,
    delayed: bool,
    reports: usize,
    delayed_reports: usize,
}

impl CodexProgressReporter {
    pub(super) fn new(socket: PathBuf, token: String, task: String) -> Self {
        Self::with_delivery(move |delayed| {
            let (code, state, text) = if delayed {
                (RuntimeProgressCode::ExternalServiceDelayed, RuntimeProgressState::Delayed,
                    "The external service is responding slowly; Codex is reconnecting or changing transport")
            } else {
                (
                    RuntimeProgressCode::ExternalServiceResumed,
                    RuntimeProgressState::Resumed,
                    "The external service responded; Codex is continuing",
                )
            };
            cli_wrapper::call_progress(
                &socket,
                CliWrapperRequest::task_status_with_progress(
                    token.clone(),
                    task.clone(),
                    text,
                    RuntimeProgressUpdate {
                        code,
                        phase: RuntimeProgressPhase::ExternalTool,
                        state,
                        tool: Some("codex".to_string()),
                        retryable: delayed,
                    },
                ),
            )
            .is_ok_and(|response| response.ok)
        })
    }

    fn with_delivery(mut deliver: impl FnMut(bool) -> bool + Send + 'static) -> Self {
        let delivered = Arc::new(AtomicUsize::new(0));
        let failed = Arc::new(AtomicUsize::new(0));
        let successes = delivered.clone();
        let failures = failed.clone();
        let dispatcher = LatestValueDispatcher::spawn("codex-progress", move |value| {
            if deliver(value) {
                successes.fetch_add(1, Ordering::Relaxed);
            } else {
                failures.fetch_add(1, Ordering::Relaxed);
            }
        })
        .ok();
        Self {
            dispatcher,
            delivered,
            failed,
            delayed: false,
            reports: 0,
            delayed_reports: 0,
        }
    }

    pub(super) fn on_line(&mut self, stderr: bool, line: &[u8]) {
        let Some(delayed) = network_event(stderr, line) else {
            return;
        };
        if self.delayed == delayed {
            return;
        }
        self.delayed = delayed;
        self.reports += 1;
        if delayed {
            self.delayed_reports += 1;
        }
        if let Some(dispatcher) = &self.dispatcher {
            dispatcher.publish(delayed);
        } else {
            self.failed.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(super) fn finish(mut self) -> Value {
        // Discard stale queued updates and finish the one bounded RPC in flight before
        // terminal handling; observation never delays pipe draining or task execution.
        if let Some(dispatcher) = self.dispatcher.take() {
            dispatcher.close();
        }
        json!({
            "schema": "awiki.generic_cli.progress_observation.v1",
            "report_attempts": self.reports,
            "report_successes": self.delivered.load(Ordering::Relaxed),
            "report_failures": self.failed.load(Ordering::Relaxed),
            "delayed_reports": self.delayed_reports,
            "delayed_at_exit": self.delayed,
        })
    }
}

fn network_event(stderr: bool, line: &[u8]) -> Option<bool> {
    if stderr {
        // Only known diagnostic prefixes; arbitrary tool output is not a network event.
        return delayed_message(std::str::from_utf8(line).ok()?.trim()).then_some(true);
    }
    let event: Value = serde_json::from_slice(line).ok()?;
    match event.get("type")?.as_str()? {
        "error" | "warning" => delayed_message(event.get("message")?.as_str()?).then_some(true),
        "turn.completed" => Some(false),
        "item.started" | "item.updated" | "item.completed" => {
            match event.get("item")?.get("type")?.as_str()? {
                "agent_message" | "reasoning" => Some(false),
                _ => None,
            }
        }
        _ => None,
    }
}

fn delayed_message(message: &str) -> bool {
    let lower = message.trim().to_ascii_lowercase();
    lower.starts_with("reconnecting...")
        || lower.starts_with("stream disconnected before completion:")
        || lower.starts_with("falling back from websockets to https")
        || lower.starts_with("falling back from websocket to https")
}

#[cfg(test)]
#[path = "codex_progress_tests.rs"]
mod tests;
