use awiki_cli::host_runtime::listener_session_lookup::{
    active_session_by_did, has_runtime_session_for_did, record_by_did, ListenerLookupManager,
    ListenerLookupRecord, ListenerLookupSession, ListenerLookupSummary,
};

#[test]
fn blank_after_trim_did_returns_none_or_false_like_go() {
    let sessions = vec![session("alice", Some(record("alice", "did:alice")))];
    let mut manager = FixtureManager::new(vec![summary("alice", "did:alice")])
        .with_record("alice", Some(record("alice", "did:alice")));

    for did in ["", " ", "\n\t"] {
        assert_eq!(active_session_by_did(&sessions, did), None);
        assert_eq!(record_by_did(did, Some(&mut manager)), None);
        assert!(!has_runtime_session_for_did(
            &sessions,
            did,
            Some(&mut manager)
        ));
    }

    assert!(manager.list_calls.is_empty());
    assert!(manager.load_calls.is_empty());
}

#[test]
fn active_session_lookup_scans_sessions_in_order_using_only_current_record_did() {
    let sessions = vec![
        session("first-no-record", None),
        session(
            "first-match",
            Some(record("ignored-record-name", "did:target")),
        ),
        session("second-match", Some(record("second", "did:target"))),
    ];

    let found = active_session_by_did(&sessions, "did:target").expect("active session");

    assert_eq!(found.identity_name, "first-match");
}

#[test]
fn active_session_lookup_uses_exact_did_match_after_nonblank_gate() {
    let sessions = vec![session("alice", Some(record("alice", "  did:alice  ")))];

    assert_eq!(active_session_by_did(&sessions, "did:alice"), None);
    assert_eq!(
        active_session_by_did(&sessions, "  did:alice  ")
            .expect("exact DID match")
            .identity_name,
        "alice"
    );
}

#[test]
fn record_lookup_returns_none_when_manager_or_list_is_unavailable() {
    assert_eq!(record_by_did("did:alice", None), None);

    let mut manager = FixtureManager::list_unavailable();
    assert_eq!(record_by_did("did:alice", Some(&mut manager)), None);
    assert_eq!(manager.list_calls, vec!["list"]);
    assert!(manager.load_calls.is_empty());
}

#[test]
fn record_lookup_scans_summaries_and_loads_only_first_matching_did() {
    let mut manager = FixtureManager::new(vec![
        summary("nonmatch", "did:other"),
        summary("first-match", "did:target"),
        summary("second-match", "did:target"),
    ])
    .with_record("first-match", Some(record("loaded-first", "did:loaded")))
    .with_record("second-match", Some(record("loaded-second", "did:loaded")));

    let found = record_by_did("did:target", Some(&mut manager)).expect("loaded record");

    assert_eq!(found.identity_name, "loaded-first");
    assert_eq!(manager.list_calls, vec!["list"]);
    assert_eq!(manager.load_calls, vec!["first-match"]);
}

#[test]
fn record_lookup_returns_none_when_first_matching_summary_load_fails() {
    let mut manager = FixtureManager::new(vec![
        summary("first-match", "did:target"),
        summary("second-match", "did:target"),
    ])
    .with_record("first-match", None)
    .with_record("second-match", Some(record("loaded-second", "did:target")));

    assert_eq!(record_by_did("did:target", Some(&mut manager)), None);
    assert_eq!(manager.load_calls, vec!["first-match"]);
}

#[test]
fn has_runtime_session_lookup_prefers_current_record_match() {
    let sessions = vec![
        session("alice", Some(record("alice", "did:target"))),
        session("bob", Some(record("bob", "did:target"))),
    ];
    let mut manager = FixtureManager::new(Vec::new())
        .with_record("alice", Some(record("alice", "did:from-load")));

    assert!(has_runtime_session_for_did(
        &sessions,
        "did:target",
        Some(&mut manager)
    ));
    assert!(manager.load_calls.is_empty());
}

#[test]
fn has_runtime_session_lookup_skips_fallback_loads_when_manager_absent() {
    let sessions = vec![
        session("alice", None),
        session("bob", Some(record("bob", "did:bob-current"))),
    ];

    assert!(!has_runtime_session_for_did(&sessions, "did:alice", None));
}

#[test]
fn has_runtime_session_lookup_loads_each_session_identity_until_match() {
    let sessions = vec![
        session("alice", Some(record("alice-current", "did:old"))),
        session("bob", None),
        session("carol", Some(record("carol-current", "did:later-current"))),
    ];
    let mut manager = FixtureManager::new(Vec::new())
        .with_record("alice", None)
        .with_record("bob", Some(record("bob", "did:target")))
        .with_record("carol", Some(record("carol", "did:target")));

    assert!(has_runtime_session_for_did(
        &sessions,
        "did:target",
        Some(&mut manager)
    ));
    assert_eq!(manager.load_calls, vec!["alice", "bob"]);
}

#[test]
fn has_runtime_session_lookup_ignores_missing_loaded_records_and_continues() {
    let sessions = vec![session("alice", None), session("bob", None)];
    let mut manager = FixtureManager::new(Vec::new())
        .with_record("alice", None)
        .with_record("bob", Some(record("bob", "did:target")));

    assert!(has_runtime_session_for_did(
        &sessions,
        "did:target",
        Some(&mut manager)
    ));
    assert_eq!(manager.load_calls, vec!["alice", "bob"]);
}

fn session(
    identity_name: &str,
    current_record: Option<ListenerLookupRecord>,
) -> ListenerLookupSession {
    ListenerLookupSession {
        identity_name: identity_name.to_string(),
        current_record,
    }
}

fn summary(identity_name: &str, did: &str) -> ListenerLookupSummary {
    ListenerLookupSummary {
        identity_name: identity_name.to_string(),
        did: did.to_string(),
    }
}

fn record(identity_name: &str, did: &str) -> ListenerLookupRecord {
    ListenerLookupRecord {
        identity_name: identity_name.to_string(),
        did: did.to_string(),
    }
}

struct FixtureManager {
    summaries: Option<Vec<ListenerLookupSummary>>,
    records: Vec<(String, Option<ListenerLookupRecord>)>,
    list_calls: Vec<&'static str>,
    load_calls: Vec<String>,
}

impl FixtureManager {
    fn new(summaries: Vec<ListenerLookupSummary>) -> Self {
        Self {
            summaries: Some(summaries),
            records: Vec::new(),
            list_calls: Vec::new(),
            load_calls: Vec::new(),
        }
    }

    fn list_unavailable() -> Self {
        Self {
            summaries: None,
            records: Vec::new(),
            list_calls: Vec::new(),
            load_calls: Vec::new(),
        }
    }

    fn with_record(mut self, identity_name: &str, record: Option<ListenerLookupRecord>) -> Self {
        self.records.push((identity_name.to_string(), record));
        self
    }
}

impl ListenerLookupManager for FixtureManager {
    fn list_summaries(&mut self) -> Option<Vec<ListenerLookupSummary>> {
        self.list_calls.push("list");
        self.summaries.clone()
    }

    fn load_record(&mut self, identity_name: &str) -> Option<ListenerLookupRecord> {
        self.load_calls.push(identity_name.to_string());
        self.records
            .iter()
            .find(|(name, _)| name == identity_name)
            .and_then(|(_, record)| record.clone())
    }
}
