use super::App;
use crate::cli_output::ExitError;
use crate::cli_parser::ParsedCommand;

impl App {
    pub async fn run_id_services_async(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        if self.globals.dry_run {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "Service commands do not support --dry-run.",
                "Use id services show to inspect the public service list.",
            ));
        }
        let input = if command.name == "id.services.update" {
            use std::io::Read;
            let path = command
                .flags
                .get("file")
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    ExitError::new(
                        "invalid_argument",
                        2,
                        "A service list file is required.",
                        "Pass --file with a JSON array of public service entries.",
                    )
                })?;
            let file = std::fs::File::open(path).map_err(|_| {
                ExitError::new(
                    "invalid_argument",
                    2,
                    "Cannot read the service list file.",
                    "Check the file path and permissions.",
                )
            })?;
            let mut bytes = Vec::new();
            file.take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| {
                    ExitError::new(
                        "invalid_argument",
                        2,
                        "Cannot read the service list file.",
                        "Check the file path and permissions.",
                    )
                })?;
            if bytes.len() > 1024 * 1024 {
                return Err(ExitError::new(
                    "invalid_argument",
                    2,
                    "The service list is too large.",
                    "The maximum file size is 1 MiB.",
                ));
            }
            Some(
                serde_json::from_slice::<Vec<im_core::identity::DidDocumentService>>(&bytes)
                    .map_err(|_| {
                        ExitError::new(
                            "invalid_argument",
                            2,
                            "The service list is invalid.",
                            "Use a JSON array containing only public DID service fields.",
                        )
                    })?,
            )
        } else {
            None
        };
        let resolved = self.resolve_config_for_workspace()?;
        let core = crate::m_core_cli_adapter::build_im_core_async(&resolved).await?;
        let registry = core.identities();
        let selector = crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity);
        let document = match command.name.as_str() {
            "id.services.show" => registry.identity_document_async(selector.clone()).await,
            "id.services.update" => {
                registry
                    .update_services_async(selector.clone(), input.unwrap_or_default())
                    .await
            }
            "id.services.resume" => {
                registry
                    .resume_services_update_async(selector.clone())
                    .await
            }
            _ => unreachable!("service handler dispatch"),
        }
        .map_err(|error| crate::m_core_cli_adapter::map_im_error(error, "id services"))?;
        let pending = registry
            .services_update_pending_async(selector)
            .await
            .map_err(|error| crate::m_core_cli_adapter::map_im_error(error, "id services"))?;
        self.render_identity_result("awiki-cli id services", &resolved, crate::m_core_cli_adapter::message_result::CommandResult {
            data: serde_json::json!({"did": document.get("id"), "services": document.get("service"), "pending": pending}),
            summary: if pending { "Service update is pending; resume the original operation" } else { "Public DID services" }.into(),
            warnings: vec![],
        })
    }
}
