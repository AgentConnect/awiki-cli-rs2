use crate::cli_output::ExitError;

pub fn unsupported_cutover_command(
    command: &str,
    capability: &str,
    required_phase: &str,
) -> ExitError {
    crate::m_core_cli_adapter::unsupported_cutover_command(command, capability, required_phase)
}
