//! Persistent-Core send measurement through the production public facade.
//! Results contain only acceptance, durations and bounded Core-wide HTTP counts.
use super::*;

pub(super) struct Params {
    group: bool,
    target_did: String,
    message_id: String,
    marker: Zeroizing<String>,
}

pub(super) fn parse(params: &Map<String, Value>) -> Result<Params, ProbeFailure> {
    require_exact_keys(params, &["kind", "marker", "message_id", "target_did"])?;
    let group = match params.get("kind").and_then(Value::as_str) {
        Some("direct") => false,
        Some("group") => true,
        _ => return Err(ProbeFailure::InvalidRequest),
    };
    Ok(Params {
        group,
        target_did: required_did(params, "target_did")?,
        message_id: required_string(params, "message_id", 512)?,
        marker: Zeroizing::new(required_opaque_string(params, "marker", 16 * 1024)?),
    })
}

fn counts(diagnostics: &im_core::network::NetworkDiagnostics) -> Value {
    let mut values = Map::new();
    for key in [
        "http_attempts",
        "did_reads",
        "prekey_publishes",
        "prekey_gets",
        "group_gets",
        "group_infos",
        "group_member_reads",
        "cipher_submissions",
        "other_http",
    ] {
        values.insert(key.into(), json!(0u64));
    }
    for record in &diagnostics.records {
        if !record.stage.starts_with("http.") {
            continue;
        }
        let key = match record.stage.as_str() {
            "http.did.resolve" => "did_reads",
            "http.direct.e2ee.publish_prekey_bundle" => "prekey_publishes",
            "http.direct.e2ee.get_prekey_bundle" => "prekey_gets",
            "http.group.get" => "group_gets",
            "http.group.get_info" => "group_infos",
            "http.group.list_members" => "group_member_reads",
            "http.direct.send" | "http.group.e2ee.send" => "cipher_submissions",
            _ => "other_http",
        };
        for field in [key, "http_attempts"] {
            values.insert(field.into(), json!(values[field].as_u64().unwrap() + 1));
        }
    }
    // The diagnostic contract is bounded to 128 records. At capacity we cannot
    // prove that no earlier attempt was evicted, so never report complete counts.
    values.insert(
        "trace_complete".into(),
        json!(diagnostics.records.len() < 128),
    );
    Value::Object(values)
}

impl Probe {
    pub(super) async fn send_encrypted_marker(
        &self,
        params: &Params,
    ) -> Result<Value, ProbeFailure> {
        let core = self._core.as_ref().ok_or(ProbeFailure::InvalidState)?;
        let client = self._client.as_ref().ok_or(ProbeFailure::InvalidState)?;
        let target = if params.group {
            im_core::messages::MessageTarget::Group(
                im_core::ids::GroupRef::parse(&params.target_did)
                    .map_err(|_| ProbeFailure::InvalidRequest)?,
            )
        } else {
            im_core::messages::MessageTarget::Direct(
                im_core::ids::PeerRef::parse(&params.target_did, "")
                    .map_err(|_| ProbeFailure::InvalidRequest)?,
            )
        };
        let preceding = counts(&core.network().diagnostics());
        core.network().clear_diagnostics_for_test();
        let started = std::time::Instant::now();
        let result = client
            .messages()
            .send_async(im_core::messages::SendMessageRequest {
                target,
                body: im_core::messages::MessageBody::Text {
                    text: params.marker.to_string(),
                    kind: im_core::messages::MessageKind::Text,
                },
                security: im_core::messages::MessageSecurityMode::E2eeRequired,
                client_message_id: Some(
                    im_core::ids::MessageId::parse(&params.message_id)
                        .map_err(|_| ProbeFailure::InvalidRequest)?,
                ),
                delivery: im_core::messages::MessageDeliveryOptions {
                    idempotency_key: Some(params.message_id.clone()),
                    wait_for_final_acceptance: true,
                },
                delegated_signing: None,
            })
            .await
            .map_err(|error| match error {
                im_core::ImError::TransportUnavailable { .. } => ProbeFailure::Transport,
                _ => ProbeFailure::Runtime,
            })?;
        if !matches!(result.delivery, im_core::messages::DeliveryState::Accepted) {
            return Err(ProbeFailure::Runtime);
        }
        let elapsed = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
        let mut measured = counts(&core.network().diagnostics());
        core.network().clear_diagnostics_for_test();
        measured["sent"] = json!(true);
        measured["elapsed_ms"] = json!(elapsed);
        measured["preceding_http_attempts"] = preceding["http_attempts"].clone();
        measured["preceding_trace_complete"] = preceding["trace_complete"].clone();
        Ok(measured)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_send_probe_rejects_plaintext_and_unrecognized_inputs() {
        let mut params = json!({"kind":"direct", "target_did":"did:example:peer", "message_id":"message", "marker":"private marker"});
        assert!(parse(params.as_object().unwrap()).is_ok());
        params["kind"] = json!("plain");
        assert!(parse(params.as_object().unwrap()).is_err());
        params["kind"] = json!("group");
        params["token"] = json!("private token");
        assert!(parse(params.as_object().unwrap()).is_err());
    }

    #[test]
    fn encrypted_send_probe_counts_are_closed_and_saturation_is_explicit() {
        let record = |stage: &str| im_core::network::NetworkDiagnostic {
            operation: 123,
            stage: stage.into(),
            elapsed_ms: 5,
            at: "private time".into(),
        };
        let mut diagnostics = im_core::network::NetworkDiagnostics {
            policy: im_core::network::NetworkPolicy::Direct,
            status: im_core::network::NetworkStatus::Direct,
            generation: 0,
            records: vec![
                record("http.did.resolve"),
                record("http.direct.send"),
                record("send.http_delivery"),
            ],
        };
        let result = counts(&diagnostics);
        assert_eq!(result["http_attempts"], 2);
        assert_eq!(result["cipher_submissions"], 1);
        assert_eq!(result["trace_complete"], true);
        assert!(!result.to_string().contains("private"));
        diagnostics.records = vec![record("http.other"); 128];
        assert_eq!(counts(&diagnostics)["trace_complete"], false);
    }
}
