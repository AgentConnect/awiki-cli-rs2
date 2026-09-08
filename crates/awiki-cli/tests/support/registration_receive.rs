//! Empty initial receive transaction for registration-oriented HTTP fixtures.
//! Its calls are asserted separately from the subsequent identity/message RPCs.
use base64::Engine;
use serde_json::{json, Value};

pub const MARKER: &str = "__DYNAMIC_REGISTRATION_RECEIVE__";

pub fn response(request: &str) -> String {
    let rpc: Value = serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
    let method = rpc["method"].as_str().unwrap();
    let result = match method {
        "anp.get_capabilities" => {
            json!({"supported_profiles": ["awiki.message-sync.explicit-negotiation.v1", "sync.snapshot_paging.v1"]})
        }
        "sync.bootstrap" => {
            let token = request
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case("authorization")
                        .then(|| value.trim().strip_prefix("Bearer "))
                        .flatten()
                })
                .expect("initial receive uses the registered device token");
            let claims: Value = serde_json::from_slice(
                &base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(token.split('.').nth(1).unwrap())
                    .unwrap(),
            )
            .unwrap();
            let requested = rpc["params"]["body"]["capabilities"]["requested_sync_capabilities"]
                .as_array()
                .unwrap()
                .clone();
            let mut lanes = serde_json::Map::new();
            for (capability, lane) in [
                ("lanes.p5_device.v1", "p5_device"),
                ("lanes.p6_group.v1", "p6_group"),
            ] {
                if requested.iter().any(|value| value == capability) {
                    lanes.insert(
                        lane.to_owned(),
                        json!({"cursor":{"stream_epoch":"1","scan_seq":"0"}, "committed_seq":"0"}),
                    );
                }
            }
            let has_p6 = lanes.contains_key("p6_group");
            let mut result = json!({
                "mode":"tail_only", "account_id":claims["user_id"], "device_id":claims["device_id"],
                "server_time":"2026-09-08T00:00:00Z", "cursor":{"stream_epoch":"1","scan_seq":"0"},
                "read_state_baseline":[], "group_state_baseline":[],
                "snapshot_capability":{"schema":3,"delivery":"paged_v1"},
                "sync_capabilities":requested, "lanes":lanes, "warnings":[]
            });
            if has_p6 {
                result["p6_delivery"] = json!({"profile":"p6.delivery_context.v1", "client_instance_id":rpc["params"]["body"]["client_instance_id"], "activated":true});
            }
            result
        }
        "sync.delta" => {
            assert_eq!(rpc["params"]["body"]["reason"], "foreground_reconcile");
            json!({"mode":"delta", "server_time":"2026-09-08T00:00:00Z", "events":[],
                "next_cursor":{"stream_epoch":"1","scan_seq":"0"}, "has_more":false,
                "recovery":null, "warnings":[]})
        }
        other => panic!("unexpected initial receive method: {other}"),
    };
    json!({"jsonrpc":"2.0", "id":rpc["id"], "result":result}).to_string()
}

pub fn completed(request: &str) -> bool {
    let rpc: Value = serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
    rpc["method"] == "sync.delta"
}
