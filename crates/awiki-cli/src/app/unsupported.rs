use crate::output::ExitError;

pub fn unsupported_cutover_command(
    command: &str,
    capability: &str,
    required_phase: &str,
) -> ExitError {
    crate::im_core_adapter::unsupported_cutover_command(command, capability, required_phase)
}
