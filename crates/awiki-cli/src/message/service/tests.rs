use super::{merge_direct_history_messages, message_identity};
use serde_json::{json, Value};

#[test]
fn merge_direct_history_messages_prefers_cache_and_orders_by_server_seq_like_go() {
    let remote = vec![
        json!({"id": "remote-old", "server_seq": 3, "content": "old remote"}),
        json!({"id": "cached-new", "server_seq": 10, "content": "stale remote"}),
        json!({"id": "same-seq-created-old", "server_seq": 10, "created_at": "2026-01-01T00:00:00Z"}),
        json!({"id": "same-seq-created-new", "server_seq": 10, "created_at": "2026-01-02T00:00:00Z"}),
    ];
    let cached = vec![
        json!({"msg_id": "cached-new", "server_seq": 10, "content": "fresh cache"}),
        json!({"msg_id": "cached-reply", "server_seq": 11, "content": "reply cache"}),
        json!({"server_seq": 12, "content": "anonymous cache"}),
    ];

    let merged = merge_direct_history_messages(&remote, cached, 0);

    assert_eq!(
        message_ids(&merged),
        [
            "",
            "cached-reply",
            "same-seq-created-new",
            "same-seq-created-old",
            "cached-new",
            "remote-old",
        ]
    );
    assert_eq!(merged[0]["content"], "anonymous cache");
    assert_eq!(merged[1]["content"], "reply cache");
    assert_eq!(merged[4]["content"], "fresh cache");

    let limited = merge_direct_history_messages(
        &remote,
        vec![
            json!({"msg_id": "cached-new", "server_seq": 10, "content": "fresh cache"}),
            json!({"msg_id": "cached-reply", "server_seq": 11, "content": "reply cache"}),
            json!({"server_seq": 12, "content": "anonymous cache"}),
        ],
        2,
    );
    assert_eq!(message_ids(&limited), ["", "cached-reply"]);
}

#[test]
fn merge_direct_history_messages_matches_go_empty_side_fast_paths() {
    let cached = vec![
        json!({"msg_id": "cached-old", "server_seq": 1}),
        json!({"msg_id": "cached-new", "server_seq": 2}),
    ];
    let remote = vec![
        json!({"id": "remote-old", "server_seq": 1}),
        json!({"id": "remote-new", "server_seq": 2}),
    ];

    assert_eq!(
        message_ids(&merge_direct_history_messages(&[], cached, 1)),
        ["cached-old"]
    );
    assert_eq!(
        message_ids(&merge_direct_history_messages(&remote, Vec::new(), 1)),
        ["remote-old", "remote-new"]
    );
}

#[test]
fn message_identity_accepts_go_client_msg_id_fallback() {
    assert_eq!(
        message_identity(&json!({"client_msg_id": "client-1"})),
        "client-1"
    );
}

fn message_ids(messages: &[Value]) -> Vec<String> {
    messages.iter().map(message_identity).collect()
}
