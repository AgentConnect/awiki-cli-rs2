use super::tools::update_summary;
use serde_json::json;

#[test]
fn tool_projection_keeps_category_and_basename_without_command_or_secrets() {
    let summary = update_summary(
        None,
        &json!({"toolCallId":"t","kind":"execute","status":"in_progress","title":"curl -H Authorization:SECRET https://host","rawInput":{"secret":"private"},"locations":[{"path":"/Users/private/work/说明.md"}]}),
    );
    assert_eq!(
        summary,
        json!({"id":"t","kind":"execute","status":"in_progress","title":"Tool","target":"说明.md"})
    );
    let complete = update_summary(
        Some(&summary),
        &json!({"toolCallId":"t","status":"completed"}),
    );
    assert_eq!(complete["target"], "说明.md");
    assert_eq!(complete["kind"], "execute");
    assert_eq!(complete["status"], "completed");
    let secret = update_summary(
        None,
        &json!({"toolCallId":"secret","title":"/tmp/sk-test-private"}),
    );
    assert!(secret["target"].is_null());
    assert_eq!(secret["title"], "Tool");
}
