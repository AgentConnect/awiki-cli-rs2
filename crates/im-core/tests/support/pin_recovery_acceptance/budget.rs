use super::*;
use serde::{Deserialize, Serialize};
use std::{io::Write, path::PathBuf, process::Command};
use zeroize::Zeroizing;
#[derive(Serialize, Deserialize)]
struct Marker {
    mode: String,
    target_id: String,
    operation_id: String,
    attempts_used: u16,
}
fn mode() -> String {
    let m = std::env::var("AWIKI_PIN_RECOVERY_BUDGET_MODE").expect("budget mode");
    assert!(matches!(m.as_str(), "last-correct" | "exhausted"));
    m
}
fn path(i: &Input, m: &str) -> PathBuf {
    i.storage_b
        .parent()
        .unwrap()
        .join(format!("acceptance-{m}"))
}
fn save(i: &Input, name: &str, v: &Marker) {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let mut f = o
        .open(path(i, &v.mode).join(name))
        .expect("new public budget marker");
    f.write_all(&serde_json::to_vec(v).unwrap())
        .expect("budget marker write");
    f.sync_all().unwrap();
}
fn command(name: &str, m: &str) -> Command {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(["--exact", name, "--ignored", "--nocapture"])
        .env("AWIKI_PIN_RECOVERY_BUDGET_MODE", m)
        .env_remove("AWIKI_PIN_RECOVERY_TEST_ENABLE")
        .env_remove("AWIKI_PIN_RECOVERY_TEST_CUT")
        .env_remove("AWIKI_PIN_RECOVERY_TEST_MARKER");
    c
}
async fn parent(m: &str) {
    let i = read_input();
    assert!(
        !path(&i, m).exists(),
        "preserve prior uncertain budget scope"
    );
    let owner = core(&i, &i.storage_a, "a");
    let before = owner_registry(&owner, "budget Registry before real children").await;
    child_evidence::run(
        &i,
        "wrong-guesses",
        &mut command("acceptance_child_budget_wrong_guesses", m),
        0,
    );
    child_evidence::run(
        &i,
        "fresh-reauthentication",
        &mut command("acceptance_child_budget_reauthenticate", m),
        0,
    );
    let done: Marker =
        serde_json::from_slice(&std::fs::read(path(&i, m).join("completed.json")).unwrap())
            .unwrap();
    assert!(
        done.mode == m
            && done.attempts_used == 10
            && uuid::Uuid::parse_str(&done.operation_id).is_ok()
    );
    assert_eq!(
        before,
        owner_registry(&owner, "budget Registry after actual process exits").await
    );
    if m == "last-correct" {
        println!("PIN-DRIVER nine_wrong_guesses=true reauthentication_preserves_budget=true last_correct_pin_recovered=true restricted_pending=true cancelled=true operation={}",done.operation_id);
    } else {
        println!("PIN-DRIVER budget_exhausted=true new_session_preserves_budget=true correct_pin_rejected_after_exhaustion=true no_identity=true registry_unchanged=true cancelled=true operation={}",done.operation_id);
    }
}
pub(super) async fn last_correct() {
    parent("last-correct").await;
}
pub(super) async fn exhausted() {
    parent("exhausted").await;
}
pub(super) async fn wrong_child() {
    let i = read_input();
    let m = mode();
    let count = if m == "last-correct" { 9 } else { 10 };
    let c = fresh_recovery(&i, &format!("acceptance-{m}")).await;
    let s = phone_session(&c, &i, None).await;
    require_sacrificial(&i, &s);
    let f = c.pin_recovery();
    let b = take(
        f.begin_recover(BeginPinRecoveryRequest {
            session: s.session_handle,
            device_label: "sacrificial paced budget".into(),
        })
        .await,
        "begin actual child budget",
    );
    assert_eq!(b.attempts_used, 0, "independent slot must start at zero");
    assert_eq!(b.attempts_limit, 10);
    println!(
        "PIN-DRIVER pending_operation=true operation={}",
        b.operation_id.0
    );
    let mut wrong = Zeroizing::new(i.pin.as_bytes().to_vec());
    assert_eq!(wrong.len(), 4);
    wrong[3] = b'0' + (wrong[3] - b'0' + 1) % 10;
    for number in 1..=count {
        if number > 1 {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
        expect_error(
            f.submit_pin(
                b.operation_id.clone(),
                take(SecretPin::new(wrong.to_vec()), "wrong PIN format"),
            )
            .await,
            &[if number == 10 {
                "guess_budget_exhausted"
            } else {
                "invalid_pin"
            }],
            "one paced explicit guess; rate_limited is not a PIN result",
        );
        let status = take(
            f.status(b.operation_id.clone()).await,
            "authoritative budget after each guess",
        );
        assert_eq!(status.attempts_used, number);
        assert!(status.recovered_identity.is_none() && status.new_device_id.is_none());
        assert!(take(c.identities().list(), "wrong child inventory").is_empty());
    }
    if m == "exhausted" {
        assert_eq!(
            take(
                f.cancel(b.operation_id.clone()).await,
                "cancel exhausted first op"
            )
            .phase,
            PinRecoveryPhase::Cancelled
        );
    }
    save(
        &i,
        "wrong-stage.json",
        &Marker {
            mode: m,
            target_id: s.target.target_id.0,
            operation_id: b.operation_id.0,
            attempts_used: count,
        },
    );
}
pub(super) async fn reauthenticate_child() {
    let i = read_input();
    let m = mode();
    let old: Marker =
        serde_json::from_slice(&std::fs::read(path(&i, &m).join("wrong-stage.json")).unwrap())
            .unwrap();
    assert!(old.mode == m && old.attempts_used == if m == "last-correct" { 9 } else { 10 });
    let c = core(&i, &path(&i, &m), &format!("acceptance-{m}"));
    assert!(take(c.identities().list(), "fresh process inventory").is_empty());
    let old_id = PinRecoveryOperationId(old.operation_id);
    let s = phone_session(
        &c,
        &i,
        if m == "last-correct" {
            Some(old_id.clone())
        } else {
            None
        },
    )
    .await;
    require_sacrificial(&i, &s);
    assert!(s.target.target_id.0 == old.target_id);
    let f = c.pin_recovery();
    let id = if m == "last-correct" {
        let p = take(
            f.resume(ResumePinRecoveryRequest {
                operation_id: old_id.clone(),
                session: Some(s.session_handle),
            })
            .await,
            "same-key fresh-process ninth guess resume",
        );
        assert_eq!(p.attempts_used, 9);
        old_id
    } else {
        let p = take(
            f.begin_recover(BeginPinRecoveryRequest {
                session: s.session_handle,
                device_label: "exhausted fresh process and PST".into(),
            })
            .await,
            "new op after actual child exit",
        );
        assert_eq!(p.attempts_used, 10);
        p.operation_id
    };
    let result = f
        .submit_pin(
            id.clone(),
            take(
                SecretPin::new(i.pin.as_bytes().to_vec()),
                "explicit correct PIN",
            ),
        )
        .await;
    if m == "last-correct" {
        let p = take(result, "tenth correct PIN after actual process restart");
        assert_eq!(p.phase, PinRecoveryPhase::AwaitingDeviceConfirmation);
        assert_eq!(p.attempts_used, 10);
        assert!(p.recovered_identity.is_none() && p.new_device_id.is_some());
    } else {
        expect_error(
            result,
            &["guess_budget_exhausted"],
            "new process/PST must not refund global budget",
        );
    }
    assert!(take(c.identities().list(), "no activated budget identity").is_empty());
    assert_eq!(
        take(f.status(id.clone()).await, "final global budget").attempts_used,
        10
    );
    assert_eq!(
        take(f.cancel(id.clone()).await, "cancel exact budget pending").phase,
        PinRecoveryPhase::Cancelled
    );
    save(
        &i,
        "completed.json",
        &Marker {
            mode: m,
            target_id: s.target.target_id.0,
            operation_id: id.0,
            attempts_used: 10,
        },
    );
}
