use super::*;
use im_core::{
    ids::{Cursor, GroupRef, MessageId, Page, PeerRef},
    messages::{MessageDirection, MessageKind, MessageMetadata, ThreadRef},
};
use std::{collections::VecDeque, future::ready};

fn message(id: &str) -> Message {
    Message {
        id: MessageId::parse(id).unwrap(),
        thread: ThreadRef::Group(GroupRef::parse("did:group:team").unwrap()),
        direction: MessageDirection::Incoming,
        sender: PeerRef::parse("did:human:member", "").unwrap(),
        receiver: None,
        group: Some(GroupRef::parse("did:group:team").unwrap()),
        body: MessageBodyView::Text {
            text: id.into(),
            kind: MessageKind::Text,
        },
        sent_at: None,
        received_at: None,
        metadata: MessageMetadata::default(),
    }
}

fn page(items: Vec<Message>, cursor: Option<&str>) -> Page<Message> {
    Page {
        items,
        has_more: cursor.is_some(),
        next_cursor: cursor.map(|s| Cursor::parse(s).unwrap()),
    }
}

#[tokio::test]
async fn history_continues_after_control_only_page_and_keeps_oldest_first() {
    let mut control = message("control");
    control.body = MessageBodyView::Payload {
        payload: json!({"schema":"awiki.acp.status.v1"}),
    };
    let mut pages = VecDeque::from([
        page(vec![control], Some("page-2")),
        page(vec![message("b"), message("a")], None),
    ]);
    let context = read_recent_group_context(&message("current"), |q| {
        if pages.len() == 1 {
            assert_eq!(q.cursor.unwrap().as_str(), "page-2");
        }
        ready(Ok(pages.pop_front().unwrap()))
    })
    .await;
    assert_eq!(context["status"], "available");
    assert_eq!(context["included_count"], 2);
    assert_eq!(context["messages"][0]["message_id"], "a");
    assert_eq!(context["messages"][1]["message_id"], "b");
}

#[tokio::test]
async fn history_empty_and_read_failure_are_distinct() {
    let current = message("current");
    let empty = read_recent_group_context(&current, |_| ready(Ok(page(vec![], None)))).await;
    assert_eq!(empty["status"], "available");
    assert_eq!(empty["included_count"], 0);
    let failure = read_recent_group_context(&current, |_| {
        ready(Err(im_core::ImError::unsupported("offline-test")))
    })
    .await;
    assert_eq!(failure["status"], "unavailable");
    assert_eq!(failure["unavailable_reason"], "local_history_unavailable");
}

#[tokio::test]
async fn history_rejects_scope_mismatch_and_cursor_loop() {
    let current = message("current");
    let mut foreign = message("foreign");
    foreign.thread = ThreadRef::Group(GroupRef::parse("did:group:other").unwrap());
    foreign.group = Some(GroupRef::parse("did:group:other").unwrap());
    let mismatch =
        read_recent_group_context(&current, |_| ready(Ok(page(vec![foreign.clone()], None)))).await;
    assert_eq!(
        mismatch["unavailable_reason"],
        "local_history_binding_mismatch"
    );
    let mut calls = 0;
    let repeated = read_recent_group_context(&current, |_| {
        calls += 1;
        ready(Ok(page(vec![], Some("same"))))
    })
    .await;
    assert_eq!(calls, 2);
    assert_eq!(
        repeated["unavailable_reason"],
        "local_history_cursor_repeated"
    );
}

#[tokio::test]
async fn history_has_bounded_scan_when_controls_dominate() {
    let mut control = message("control");
    control.body = MessageBodyView::Payload {
        payload: json!({"schema":"awiki.acp.status.v1"}),
    };
    let mut calls = 0;
    let context = read_recent_group_context(&message("current"), |_| {
        calls += 1;
        ready(Ok(page(
            vec![control.clone(); 100],
            Some(&format!("page-{calls}")),
        )))
    })
    .await;
    assert_eq!(calls, 10);
    assert_eq!(context["status"], "available");
    assert_eq!(context["included_count"], 0);
    assert_eq!(context["scan_limit_reached"], true);
}
