use serde_json::json;

use crate::cli_output::{ErrorDetail, ExitError};

pub fn unsupported_cutover_command(
    command: &str,
    capability: &str,
    required_phase: &str,
) -> ExitError {
    ExitError {
        exit_code: 2,
        detail: ErrorDetail {
            code: "unsupported_capability".to_string(),
            message: format!(
                "{capability} are not supported by the im-core CLI cutover path for {command}."
            ),
            hint: format!(
                "Use a supported high-level command now, or enable this capability after {required_phase} lands."
            ),
            retryable: false,
            details: json!({
                "command": command,
                "capability": capability,
                "required_phase": required_phase,
                "cutover_status": "unsupported",
            }),
        },
    }
}
