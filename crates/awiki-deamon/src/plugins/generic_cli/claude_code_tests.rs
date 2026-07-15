use std::path::PathBuf;
use std::time::Duration;

use serde_json::json;

use super::{
    external_tool_result_id, external_tool_result_is_temporary_error, external_tool_start,
    ClaudeCodeProgressReporter,
};

#[test]
fn parses_supported_external_tool_lifecycle_from_stream_json() {
    let start = json!({
        "type": "assistant",
        "message": {
            "content": [{
                "type": "tool_use",
                "id": "tool_1",
                "name": "WebSearch",
                "input": {"query": "weather"}
            }]
        }
    });
    let result = json!({
        "type": "user",
        "message": {
            "content": [{
                "type": "tool_result",
                "tool_use_id": "tool_1",
                "content": "done"
            }]
        }
    });

    assert_eq!(
        external_tool_start(&start),
        Some(("tool_1".to_string(), "web_search".to_string()))
    );
    assert_eq!(external_tool_result_id(&result).as_deref(), Some("tool_1"));
}

#[test]
fn classifies_temporary_upstream_tool_errors_without_claiming_recovery() {
    let result = json!({
        "type": "user",
        "message": {
            "content": [{
                "type": "tool_result",
                "tool_use_id": "tool_1",
                "content": "API Error: 502 BadGatewayError: Upstream request failed (upstream_error)"
            }]
        }
    });

    assert!(external_tool_result_is_temporary_error(&result));
}

#[test]
fn delayed_external_tool_reports_are_thresholded_and_throttled() {
    let mut reporter = ClaudeCodeProgressReporter::new(
        PathBuf::from("/missing/daemon.sock"),
        "rtok_test".to_string(),
        "task_1".to_string(),
    );
    let start = json!({
        "message": {
            "content": [{
                "type": "tool_use",
                "id": "tool_1",
                "name": "WebFetch"
            }]
        }
    });
    reporter.on_stdout_line(start.to_string().as_bytes(), Duration::ZERO);
    assert_eq!(reporter.report_attempts, 1);

    reporter.on_tick(Duration::from_secs(14));
    assert_eq!(reporter.delayed_reports, 0);
    reporter.on_tick(Duration::from_secs(15));
    assert_eq!(reporter.delayed_reports, 1);
    reporter.on_tick(Duration::from_secs(74));
    assert_eq!(reporter.delayed_reports, 1);
    reporter.on_tick(Duration::from_secs(75));
    assert_eq!(reporter.delayed_reports, 2);

    let result = json!({
        "message": {
            "content": [{
                "type": "tool_result",
                "tool_use_id": "tool_1"
            }]
        }
    });
    reporter.on_stdout_line(result.to_string().as_bytes(), Duration::from_secs(76));
    assert!(reporter.active_external_tool.is_none());
    assert_eq!(reporter.report_attempts, 4);
    assert_eq!(reporter.report_failures, 4);
}
