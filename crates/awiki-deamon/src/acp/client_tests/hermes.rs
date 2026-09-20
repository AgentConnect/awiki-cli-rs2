use super::*;

#[tokio::test]
async fn hermes_load_never_resumes_or_silently_replaces_a_session() {
    for (mode, success, lost) in [
        ("", true, false),
        ("hermes-provenance", true, false),
        ("hermes-listed", true, false),
        ("hermes-null", false, true),
        ("hermes-wrong", false, false),
        ("list-error", false, false),
        ("list-malformed", false, false),
        ("list-loop", false, false),
        ("list-present", false, false),
    ] {
        let mut f = Fixture::new();
        f.profile.driver_id = "hermes".into();
        f.profile.config_home = Some(f.root.path().join("hermes-home"));
        run(f.turn("hello")).await.unwrap();
        f.next();
        std::fs::write(f.root.path().join("work/recovery-mode"), mode).unwrap();
        let result = run(f.turn("again")).await;
        assert_eq!(result.is_ok(), success, "{mode}");
        if let Ok(result) = result {
            assert_eq!(result.text, "FIXTURE_RESPONSE");
        }
        let session = store::load(&f.state, &f.key).unwrap();
        assert_eq!(session.context_lost, lost, "{mode}");
        assert_eq!(
            session.native_session_id.as_deref(),
            Some("native-exact-session")
        );
        let log = std::fs::read_to_string(f.root.path().join("work/protocol.jsonl")).unwrap();
        assert_eq!(log.matches("session/new").count(), 1, "{mode}");
        assert_eq!(log.matches("session/load").count(), 1, "{mode}");
        assert!(!log.contains("session/resume"), "{mode}");
    }
}

#[tokio::test]
async fn hermes_model_preparation_requires_the_same_strict_restoration() {
    let mut f = Fixture::new();
    f.profile.driver_id = "hermes".into();
    f.profile.config_home = Some(f.root.path().join("hermes-home"));
    let cwd = f.root.path().join("work");
    std::fs::write(cwd.join("recovery-mode"), "hermes-null").unwrap();
    let failure = prepare_configuration(
        f.profile.clone(),
        cwd.clone(),
        Some("native-exact-session".into()),
        None,
    )
    .await;
    assert_eq!(failure.err().unwrap().to_string(), "context_reset_required");
    std::fs::write(cwd.join("recovery-mode"), "hermes-provenance").unwrap();
    let result = prepare_configuration(
        f.profile,
        cwd.clone(),
        Some("native-exact-session".into()),
        None,
    )
    .await
    .unwrap();
    assert_eq!(current_model(&result.options).as_deref(), Some("flash"));
    assert!(!cwd.join("prompts.jsonl").exists());
    let log = std::fs::read_to_string(cwd.join("protocol.jsonl")).unwrap();
    assert!(!log.contains("session/new") && !log.contains("session/resume"));
}
