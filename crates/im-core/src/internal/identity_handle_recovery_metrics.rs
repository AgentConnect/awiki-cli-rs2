//! Secret-free, bounded Handle Recovery V4 counters and age gauges.

use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) const RECOVERY_REMOTE_UNRESOLVED_AGE_SECONDS: &str =
    "recovery_remote_unresolved_age_seconds";
pub(crate) const RECOVERY_KEY_UNAVAILABLE_TOTAL: &str = "recovery_key_unavailable_total";
pub(crate) const RECOVERY_BREAK_GLASS_TOTAL: &str = "recovery_break_glass_total";
pub(crate) const RECOVERY_LOCAL_TRANSITION_PENDING_AGE_SECONDS: &str =
    "recovery_local_transition_pending_age_seconds";
pub(crate) const GROUP_REPAIR_TOTAL: &str = "group_repair_total";

const BREAK_GLASS_RESULTS: usize = 3;
const GROUP_REPAIR_RESULTS: usize = 4;

static REMOTE_UNRESOLVED_AGE_SECONDS: AtomicU64 = AtomicU64::new(0);
static KEY_UNAVAILABLE_TOTAL: AtomicU64 = AtomicU64::new(0);
static BREAK_GLASS_TOTAL: [AtomicU64; BREAK_GLASS_RESULTS] =
    [const { AtomicU64::new(0) }; BREAK_GLASS_RESULTS];
static LOCAL_TRANSITION_PENDING_AGE_SECONDS: AtomicU64 = AtomicU64::new(0);
static GROUP_REPAIR_TOTALS: [AtomicU64; GROUP_REPAIR_RESULTS] =
    [const { AtomicU64::new(0) }; GROUP_REPAIR_RESULTS];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BreakGlassResult {
    Authorized,
    Rejected,
    Applied,
}

impl BreakGlassResult {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Authorized => "authorized",
            Self::Rejected => "rejected",
            Self::Applied => "applied",
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GroupRepairResult {
    Completed,
    Pending,
    Blocked,
    Noop,
}

impl GroupRepairResult {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Pending => "pending",
            Self::Blocked => "blocked",
            Self::Noop => "noop",
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

pub(crate) fn record_remote_unresolved_age(seconds: u64) {
    REMOTE_UNRESOLVED_AGE_SECONDS.store(seconds, Ordering::Relaxed);
}

pub(crate) fn record_key_unavailable() {
    KEY_UNAVAILABLE_TOTAL.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn record_break_glass(result: BreakGlassResult) {
    BREAK_GLASS_TOTAL[result.index()].fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn record_local_transition_pending_age(seconds: u64) {
    LOCAL_TRANSITION_PENDING_AGE_SECONDS.store(seconds, Ordering::Relaxed);
}

pub(crate) fn record_group_repair(result: GroupRepairResult) {
    GROUP_REPAIR_TOTALS[result.index()].fetch_add(1, Ordering::Relaxed);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HandleRecoveryMetricsSnapshot {
    pub(crate) remote_unresolved_age_seconds: u64,
    pub(crate) key_unavailable_total: u64,
    pub(crate) break_glass_total: [u64; BREAK_GLASS_RESULTS],
    pub(crate) local_transition_pending_age_seconds: u64,
    pub(crate) group_repair_total: [u64; GROUP_REPAIR_RESULTS],
}

pub(crate) fn snapshot() -> HandleRecoveryMetricsSnapshot {
    HandleRecoveryMetricsSnapshot {
        remote_unresolved_age_seconds: REMOTE_UNRESOLVED_AGE_SECONDS.load(Ordering::Relaxed),
        key_unavailable_total: KEY_UNAVAILABLE_TOTAL.load(Ordering::Relaxed),
        break_glass_total: std::array::from_fn(|index| {
            BREAK_GLASS_TOTAL[index].load(Ordering::Relaxed)
        }),
        local_transition_pending_age_seconds: LOCAL_TRANSITION_PENDING_AGE_SECONDS
            .load(Ordering::Relaxed),
        group_repair_total: std::array::from_fn(|index| {
            GROUP_REPAIR_TOTALS[index].load(Ordering::Relaxed)
        }),
    }
}

pub(crate) fn public_snapshot() -> crate::identity::HandleRecoveryMetricsSnapshot {
    let snapshot = snapshot();
    crate::identity::HandleRecoveryMetricsSnapshot {
        recovery_remote_unresolved_age_seconds: snapshot.remote_unresolved_age_seconds,
        recovery_key_unavailable_total: snapshot.key_unavailable_total,
        recovery_break_glass_total: [
            BreakGlassResult::Authorized,
            BreakGlassResult::Rejected,
            BreakGlassResult::Applied,
        ]
        .into_iter()
        .map(|result| {
            (
                result.as_str().to_owned(),
                snapshot.break_glass_total[result.index()],
            )
        })
        .collect(),
        recovery_local_transition_pending_age_seconds: snapshot
            .local_transition_pending_age_seconds,
        group_repair_total: [
            GroupRepairResult::Completed,
            GroupRepairResult::Pending,
            GroupRepairResult::Blocked,
            GroupRepairResult::Noop,
        ]
        .into_iter()
        .map(|result| {
            (
                result.as_str().to_owned(),
                snapshot.group_repair_total[result.index()],
            )
        })
        .collect(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn metrics_have_only_closed_secret_free_dimensions() {
        assert_eq!(
            [
                super::BreakGlassResult::Authorized.as_str(),
                super::BreakGlassResult::Rejected.as_str(),
                super::BreakGlassResult::Applied.as_str(),
            ],
            ["authorized", "rejected", "applied"]
        );
        assert_eq!(
            [
                super::GroupRepairResult::Completed.as_str(),
                super::GroupRepairResult::Pending.as_str(),
                super::GroupRepairResult::Blocked.as_str(),
                super::GroupRepairResult::Noop.as_str(),
            ],
            ["completed", "pending", "blocked", "noop"]
        );
        for metric in [
            super::RECOVERY_REMOTE_UNRESOLVED_AGE_SECONDS,
            super::RECOVERY_KEY_UNAVAILABLE_TOTAL,
            super::RECOVERY_BREAK_GLASS_TOTAL,
            super::RECOVERY_LOCAL_TRANSITION_PENDING_AGE_SECONDS,
            super::GROUP_REPAIR_TOTAL,
        ] {
            assert!(!metric.contains("did"));
            assert!(!metric.contains("operation"));
        }
        let _ = super::snapshot();
        let public = super::public_snapshot();
        assert_eq!(
            public.recovery_break_glass_total.keys().collect::<Vec<_>>(),
            ["applied", "authorized", "rejected"]
        );
        assert_eq!(
            public.group_repair_total.keys().collect::<Vec<_>>(),
            ["blocked", "completed", "noop", "pending"]
        );
    }
}
