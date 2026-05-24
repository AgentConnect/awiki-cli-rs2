#![allow(dead_code)]

use serde_json::{Map, Value};

pub(crate) fn direct_init_body_from_value(value: &Value) -> anp::direct_e2ee::DirectInitBody {
    let object = value.as_object();
    anp::direct_e2ee::DirectInitBody {
        session_id: string_value(object.and_then(|value| value.get("session_id"))),
        suite: string_value(object.and_then(|value| value.get("suite"))),
        sender_static_key_agreement_id: string_value(
            object.and_then(|value| value.get("sender_static_key_agreement_id")),
        ),
        recipient_bundle_id: string_value(
            object.and_then(|value| value.get("recipient_bundle_id")),
        ),
        recipient_signed_prekey_id: string_value(
            object.and_then(|value| value.get("recipient_signed_prekey_id")),
        ),
        recipient_one_time_prekey_id: optional_nonempty_string(
            object.and_then(|value| value.get("recipient_one_time_prekey_id")),
        ),
        sender_ephemeral_pub_b64u: string_value(
            object.and_then(|value| value.get("sender_ephemeral_pub_b64u")),
        ),
        ciphertext_b64u: string_value(object.and_then(|value| value.get("ciphertext_b64u"))),
    }
}

pub(crate) fn direct_cipher_body_from_value(value: &Value) -> anp::direct_e2ee::DirectCipherBody {
    let object = value.as_object();
    let ratchet_header = object
        .and_then(|value| value.get("ratchet_header"))
        .and_then(Value::as_object);
    anp::direct_e2ee::DirectCipherBody {
        session_id: string_value(object.and_then(|value| value.get("session_id"))),
        suite: optional_nonempty_string(object.and_then(|value| value.get("suite"))),
        ratchet_header: anp::direct_e2ee::RatchetHeader {
            dh_pub_b64u: string_value(ratchet_header.and_then(|value| value.get("dh_pub_b64u"))),
            pn: string_value(ratchet_header.and_then(|value| value.get("pn"))),
            n: string_value(ratchet_header.and_then(|value| value.get("n"))),
        },
        ciphertext_b64u: string_value(object.and_then(|value| value.get("ciphertext_b64u"))),
    }
}

pub(crate) fn sort_history_messages_for_decrypt(messages: &mut [Map<String, Value>]) {
    messages.sort_by(compare_history_messages);
}

pub(crate) fn compare_history_messages(
    left: &Map<String, Value>,
    right: &Map<String, Value>,
) -> std::cmp::Ordering {
    let left_seq = history_server_seq(left);
    let right_seq = history_server_seq(right);
    left_seq
        .partial_cmp(&right_seq)
        .unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| history_message_id(left).cmp(&history_message_id(right)))
}

fn history_server_seq(message: &Map<String, Value>) -> f64 {
    message
        .get("server_seq")
        .and_then(Value::as_f64)
        .unwrap_or_default()
}

fn history_message_id(message: &Map<String, Value>) -> String {
    message
        .get("meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("message_id"))
        .map(string_value_from_value)
        .unwrap_or_default()
}

fn string_value(value: Option<&Value>) -> String {
    value.map(string_value_from_value).unwrap_or_default()
}

fn string_value_from_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn optional_nonempty_string(value: Option<&Value>) -> Option<String> {
    let value = string_value(value);
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn direct_init_body_parses_legacy_go_map_shapes() {
        let body = direct_init_body_from_value(&json!({
            "session_id": "session-1",
            "suite": "suite-1",
            "sender_static_key_agreement_id": "did:alice#key-3",
            "recipient_bundle_id": "bundle-1",
            "recipient_signed_prekey_id": "spk-1",
            "recipient_one_time_prekey_id": "",
            "sender_ephemeral_pub_b64u": "ephemeral",
            "ciphertext_b64u": "cipher",
        }));

        assert_eq!(body.session_id, "session-1");
        assert_eq!(body.recipient_one_time_prekey_id, None);
        assert_eq!(body.sender_static_key_agreement_id, "did:alice#key-3");
    }

    #[test]
    fn direct_cipher_body_parses_ratchet_header_and_stringifies_scalars() {
        let body = direct_cipher_body_from_value(&json!({
            "session_id": "session-1",
            "suite": 7,
            "ratchet_header": {
                "dh_pub_b64u": "dh",
                "pn": 1,
                "n": true,
            },
            "ciphertext_b64u": "cipher",
        }));

        assert_eq!(body.session_id, "session-1");
        assert_eq!(body.suite.as_deref(), Some("7"));
        assert_eq!(body.ratchet_header.dh_pub_b64u, "dh");
        assert_eq!(body.ratchet_header.pn, "1");
        assert_eq!(body.ratchet_header.n, "true");
    }

    #[test]
    fn sort_history_messages_uses_server_seq_then_message_id() {
        let mut messages = vec![
            history_message(2.0, "msg-c"),
            history_message(1.0, "msg-b"),
            history_message(1.0, "msg-a"),
        ];

        sort_history_messages_for_decrypt(&mut messages);

        let ids = messages.iter().map(history_id).collect::<Vec<_>>();
        assert_eq!(ids, vec!["msg-a", "msg-b", "msg-c"]);
    }

    fn history_message(seq: f64, id: &str) -> Map<String, Value> {
        let value = json!({
            "server_seq": seq,
            "meta": {
                "message_id": id,
            },
        });
        value.as_object().cloned().unwrap()
    }

    fn history_id(message: &Map<String, Value>) -> String {
        message
            .get("meta")
            .and_then(Value::as_object)
            .and_then(|meta| meta.get("message_id"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    }
}
