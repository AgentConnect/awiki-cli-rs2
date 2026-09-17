use super::*;

#[test]
fn failure_delay_starts_after_failure_and_third_attempt_can_succeed() {
    let mut task = ManagementTask::authorized();
    assert!(!task.claim(0));
    task.activate();
    assert!(task.claim(0));
    assert!(!task.claim(1));
    task.failed_attempt(8_000, "prekey_unavailable", true);
    assert!(!task.claim(12_999));
    assert!(task.claim(13_000));
    task.failed_attempt(20_000, "prekey_unavailable", true);
    assert!(!task.claim(24_999));
    assert!(task.claim(25_000));
    task.accepted();
    assert_eq!(task.attempts, 3);
    assert_eq!(task.phase, ManagementPhase::WaitingForRecipient);
    assert!(!task.claim(i64::MAX));
}

#[test]
fn crashes_do_not_refund_budget_or_allow_a_fourth_attempt() {
    let mut task = ManagementTask::authorized();
    task.activate();
    for n in 0..3 {
        assert!(task.claim(n * 10_000));
        let saved = serde_json::to_vec(&task).unwrap();
        task = serde_json::from_slice(&saved).unwrap();
        task.recover_interrupted(n * 10_000 + 1_000);
        assert!(!task.claim(n * 10_000 + 5_999));
    }
    assert_eq!(task.attempts, 3);
    assert_eq!(task.phase, ManagementPhase::Failed);
    assert!(!task.claim(i64::MAX));
    task.activate();
    assert!(!task.claim(i64::MAX));
}

#[test]
fn terminal_validation_failure_stops_even_with_remaining_budget() {
    let mut task = ManagementTask::authorized();
    task.activate();
    assert!(task.claim(0));
    task.failed_attempt(500, "key_changed", false);
    assert_eq!(task.phase, ManagementPhase::Failed);
    assert!(!task.claim(10_000));
}

#[test]
fn first_acceptance_never_claims_another_send_or_implies_management_ready() {
    let mut task = ManagementTask::authorized();
    task.activate();
    assert!(task.claim(0));
    task.accepted();
    task.recover_interrupted(100_000);
    assert_eq!(task.attempts, 1);
    assert_eq!(task.phase, ManagementPhase::WaitingForRecipient);
    assert!(!task.claim(100_000));
}

struct FakeIo {
    now: i64,
    saved: Option<ManagementTask>,
    outcomes: std::collections::VecDeque<crate::identity::RootKeyTransferResult<()>>,
    sends: Vec<i64>,
    local_accepted: bool,
    remote_registered: bool,
    persist_fails: bool,
    expired: bool,
}
impl FakeIo {
    fn new(codes: &[Option<crate::identity::RootKeyTransferErrorCode>]) -> Self {
        Self {
            now: 0,
            saved: None,
            outcomes: codes
                .iter()
                .map(|c| {
                    c.map_or(Ok(()), |c| {
                        Err(crate::identity::RootKeyTransferError::new(c))
                    })
                })
                .collect(),
            sends: vec![],
            local_accepted: false,
            remote_registered: false,
            persist_fails: false,
            expired: false,
        }
    }
}
#[async_trait::async_trait]
impl TaskIo for FakeIo {
    fn now_ms(&self) -> i64 {
        self.now
    }
    fn accepted_locally(&mut self) -> crate::ImResult<bool> {
        Ok(self.local_accepted)
    }
    fn delivery_expired(&mut self) -> crate::ImResult<bool> {
        Ok(self.expired)
    }
    fn persist(&mut self, task: &ManagementTask) -> crate::ImResult<()> {
        if self.persist_fails {
            return Err(crate::ImError::PermissionDenied);
        }
        // Exercise the persisted representation, not shared in-memory aliases.
        self.saved = Some(serde_json::from_slice(&serde_json::to_vec(task).unwrap()).unwrap());
        Ok(())
    }
    async fn registered(&mut self) -> crate::identity::RootKeyTransferResult<bool> {
        Ok(self.remote_registered)
    }
    async fn send(&mut self) -> crate::identity::RootKeyTransferResult<()> {
        assert_eq!(
            self.saved.as_ref().unwrap().phase,
            ManagementPhase::Attempting
        );
        self.sends.push(self.now);
        self.now += 700; // completion, not start, anchors retry delay
        let result = self.outcomes.pop_front().expect("no unbudgeted send");
        if result.is_ok() {
            self.local_accepted = true;
        }
        result
    }
}

#[tokio::test]
async fn production_driver_prekey_failures_then_success_obey_completion_delay() {
    use crate::identity::RootKeyTransferErrorCode::PrekeyUnavailable;
    let mut io = FakeIo::new(&[Some(PrekeyUnavailable), Some(PrekeyUnavailable), None]);
    let mut task = ManagementTask::authorized();
    advance_task(&mut task, &mut io).await.unwrap();
    assert_eq!(task.next_attempt_at_ms, 5_700);
    io.now = 5_699;
    advance_task(&mut task, &mut io).await.unwrap();
    assert_eq!(io.sends, [0]);
    io.now = 5_700;
    advance_task(&mut task, &mut io).await.unwrap();
    io.now = 11_400;
    advance_task(&mut task, &mut io).await.unwrap();
    assert_eq!(io.sends, [0, 5_700, 11_400]);
    assert_eq!(task.phase, ManagementPhase::WaitingForRecipient);
    io.now = 100_000;
    advance_task(&mut task, &mut io).await.unwrap();
    assert_eq!(io.sends.len(), 3);
}

#[tokio::test]
async fn lost_response_after_last_attempt_reconciles_registered_without_resend() {
    use crate::identity::RootKeyTransferErrorCode::TransportPending;
    let mut io = FakeIo::new(&[Some(TransportPending); 3]);
    let mut task = ManagementTask::authorized();
    for now in [0, 5_700, 11_400] {
        io.now = now;
        advance_task(&mut task, &mut io).await.unwrap();
    }
    assert_eq!(task.phase, ManagementPhase::Failed);
    task = io.saved.clone().unwrap();
    io.remote_registered = true;
    advance_task(&mut task, &mut io).await.unwrap();
    assert_eq!(task.phase, ManagementPhase::ManagementRegistered);
    assert_eq!(task.attempts, 3);
    assert_eq!(io.sends.len(), 3);
}

#[tokio::test]
async fn durable_charge_failure_prevents_any_transport_work() {
    let mut io = FakeIo::new(&[None]);
    io.persist_fails = true;
    let mut task = ManagementTask::authorized();
    assert!(advance_task(&mut task, &mut io).await.is_err());
    assert!(io.sends.is_empty());
}

#[tokio::test]
async fn accepted_ledger_prevents_resend_after_response_persistence_crash() {
    let mut io = FakeIo::new(&[]);
    io.local_accepted = true;
    let mut task = ManagementTask::authorized();
    task.activate();
    assert!(task.claim(0));
    advance_task(&mut task, &mut io).await.unwrap();
    assert_eq!(task.phase, ManagementPhase::WaitingForRecipient);
    assert_eq!(task.attempts, 1);
    assert!(io.sends.is_empty());
}

#[tokio::test]
async fn manual_retry_reconciles_acceptance_before_starting_a_new_bounded_round() {
    let mut exhausted = ManagementTask::authorized();
    exhausted.phase = ManagementPhase::Failed;
    exhausted.attempts = 3;
    for (registered, accepted) in [(true, false), (false, true)] {
        let mut task = exhausted.clone();
        let mut io = FakeIo::new(&[]);
        io.remote_registered = registered;
        io.local_accepted = accepted;
        retry_task(&mut task, &mut io).await.unwrap();
        assert_eq!(task.attempts, 3);
        assert_eq!(
            task.phase,
            if registered {
                ManagementPhase::ManagementRegistered
            } else {
                ManagementPhase::WaitingForRecipient
            }
        );
        assert!(io.sends.is_empty());
    }
    let mut io = FakeIo::new(&[None]);
    retry_task(&mut exhausted, &mut io).await.unwrap();
    assert_eq!(exhausted.attempts, 0);
    assert!(io.sends.is_empty());
    advance_task(&mut exhausted, &mut io).await.unwrap();
    assert_eq!(exhausted.attempts, 1);
    assert_eq!(io.sends.len(), 1);
}

#[tokio::test]
async fn expired_delivery_stops_waiting_and_cannot_refund_budget_or_resend() {
    for accepted in [false, true] {
        let mut io = FakeIo::new(&[]);
        io.local_accepted = accepted;
        io.expired = true;
        let mut task = ManagementTask::authorized();
        task.attempts = 2;
        task.phase = if accepted {
            ManagementPhase::WaitingForRecipient
        } else {
            ManagementPhase::Scheduled
        };
        advance_task(&mut task, &mut io).await.unwrap();
        assert_eq!(task.phase, ManagementPhase::Failed);
        assert_eq!(
            task.failure_code.as_deref(),
            Some("root_transfer.delivery_expired")
        );
        let mut restored = io.saved.clone().unwrap();
        assert!(retry_task(&mut restored, &mut io).await.is_err());
        advance_task(&mut restored, &mut io).await.unwrap();
        assert_eq!(restored.phase, ManagementPhase::Failed);
        assert_eq!(restored.attempts, 2);
        assert!(io.sends.is_empty());
        // A lost response is not proof that the recipient did not import.
        io.remote_registered = true;
        advance_task(&mut restored, &mut io).await.unwrap();
        assert_eq!(restored.phase, ManagementPhase::ManagementRegistered);
        assert!(io.sends.is_empty());
    }
}
