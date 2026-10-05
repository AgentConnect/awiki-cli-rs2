use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use im_core::site::SiteDomain;
use im_core::tenant_registration::{
    AllowlistContact, ConfigureRegistration, ContactType, ListRegistrationEntries, RegistrationMode,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::App;
use crate::cli_output::ExitError;
use crate::cli_parser::ParsedCommand;

const PLACEHOLDER_OPERATION: &str = "11111111-1111-4111-8111-111111111111";

enum Action {
    Show(SiteDomain),
    List(ListRegistrationEntries),
    Configure(ConfigureRegistration),
    Operation(SiteDomain, String),
    Export(SiteDomain, String),
}

impl App {
    pub fn run_tenant_registration(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let action = parse_action(command)?;
        let resolved = self.resolve_config_for_workspace()?;
        if self.globals.dry_run {
            return self.render_success(
                &command.trace_command(),
                &resolved,
                dry_plan(&action),
                "Domain registration operation prepared; no request sent",
                vec![],
            );
        }
        let client = crate::m_core_cli_adapter::build_im_client(
            &resolved,
            crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
        )?;
        let api = client.tenant_registration();
        let value = match action {
            Action::Show(domain) => serialize(api.get_policy(domain).map_err(map_error)?)?,
            Action::List(query) => serialize(api.list_entries(query).map_err(map_error)?)?,
            Action::Operation(domain, operation) => {
                serialize(api.get_operation(domain, &operation).map_err(map_error)?)?
            }
            Action::Configure(mut change) => {
                bind_operation(&resolved, &client, command, &mut change)?;
                serialize(api.configure(change).map_err(map_error)?)?
            }
            Action::Export(domain, path) => {
                let mut after = None;
                let mut revision = None;
                let mut contacts = Vec::new();
                loop {
                    let page = api
                        .list_entries(ListRegistrationEntries {
                            domain: domain.clone(),
                            limit: 100,
                            after: after.clone(),
                            contact_type: None,
                            include_values: true,
                        })
                        .map_err(map_error)?;
                    append_export(&mut contacts, &mut revision, &page)?;
                    after = page.next_cursor;
                    if after.is_none() {
                        break;
                    }
                }
                finish_export(&domain, &path, revision, &contacts)?
            }
        };
        self.render_success(
            &command.trace_command(),
            &resolved,
            value,
            "Domain registration operation completed",
            vec![],
        )
    }

    pub async fn run_tenant_registration_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        if self.globals.dry_run {
            return self.run_tenant_registration(command);
        }
        let action = parse_action(command)?;
        let resolved = self.resolve_config_for_workspace()?;
        let client = crate::m_core_cli_adapter::build_im_client_async(
            &resolved,
            crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity),
        )
        .await?;
        let api = client.tenant_registration();
        let value = match action {
            Action::Show(domain) => {
                serialize(api.get_policy_async(domain).await.map_err(map_error)?)?
            }
            Action::List(query) => {
                serialize(api.list_entries_async(query).await.map_err(map_error)?)?
            }
            Action::Operation(domain, operation) => serialize(
                api.get_operation_async(domain, &operation)
                    .await
                    .map_err(map_error)?,
            )?,
            Action::Configure(mut change) => {
                bind_operation(&resolved, &client, command, &mut change)?;
                serialize(api.configure_async(change).await.map_err(map_error)?)?
            }
            Action::Export(domain, path) => {
                let mut after = None;
                let mut revision = None;
                let mut contacts = Vec::new();
                loop {
                    let page = api
                        .list_entries_async(ListRegistrationEntries {
                            domain: domain.clone(),
                            limit: 100,
                            after: after.clone(),
                            contact_type: None,
                            include_values: true,
                        })
                        .await
                        .map_err(map_error)?;
                    append_export(&mut contacts, &mut revision, &page)?;
                    after = page.next_cursor;
                    if after.is_none() {
                        break;
                    }
                }
                finish_export(&domain, &path, revision, &contacts)?
            }
        };
        self.render_success(
            &command.trace_command(),
            &resolved,
            value,
            "Domain registration operation completed",
            vec![],
        )
    }
}

fn parse_action(command: &ParsedCommand) -> Result<Action, ExitError> {
    let domain = SiteDomain::parse(required(command, "domain")?).map_err(map_error)?;
    match command.name.as_str() {
        "site.registration.show" => Ok(Action::Show(domain)),
        "site.registration.operation" => Ok(Action::Operation(
            domain,
            required(command, "operation-id")?,
        )),
        "site.registration.entries.list" => Ok(Action::List(ListRegistrationEntries {
            domain,
            limit: flag(command, "limit")
                .unwrap_or_else(|| "100".into())
                .parse()
                .map_err(|_| invalid("Invalid page limit"))?,
            after: flag(command, "after"),
            contact_type: flag(command, "type")
                .map(|v| contact_type(&v))
                .transpose()?,
            include_values: false,
        })),
        "site.registration.entries.export" => {
            let path = required(command, "output")?;
            if Path::new(&path).exists() {
                return Err(invalid("Export destination already exists"));
            }
            Ok(Action::Export(domain, path))
        }
        "site.registration.configure"
        | "site.registration.entries.add"
        | "site.registration.entries.remove" => {
            let file_flag = if command.name == "site.registration.entries.add" {
                "file"
            } else {
                "entries-file"
            };
            let additions = if command.name == "site.registration.entries.add"
                || flag(command, file_flag).is_some()
            {
                let path = required(command, file_flag)?;
                serde_json::from_slice::<Vec<AllowlistContact>>(&private_read(
                    Path::new(&path),
                    65536,
                )?)
                .map_err(|_| {
                    invalid("Contact file must be a JSON array of contact_type/value objects")
                })?
            } else {
                vec![]
            };
            let remove = if command.name == "site.registration.entries.remove"
                || flag(command, "entry-id").is_some()
            {
                vec![required(command, "entry-id")?]
            } else {
                vec![]
            };
            let mut change = ConfigureRegistration {
                domain,
                operation_id: flag(command, "operation-id")
                    .unwrap_or_else(|| PLACEHOLDER_OPERATION.into()),
                expected_revision: required(command, "expected-revision")?
                    .parse()
                    .map_err(|_| invalid("Invalid expected revision"))?,
                mode: flag(command, "mode")
                    .map(|value| match value.as_str() {
                        "open" => Ok(RegistrationMode::Open),
                        "allowlist" => Ok(RegistrationMode::Allowlist),
                        _ => Err(invalid("Mode must be open or allowlist")),
                    })
                    .transpose()?,
                allowed_contact_types: flag(command, "methods")
                    .map(|value| {
                        value
                            .split(',')
                            .map(|value| contact_type(value.trim()))
                            .collect()
                    })
                    .transpose()?,
                add_entries: additions,
                remove_entry_ids: remove,
            };
            change.normalize().map_err(map_error)?;
            Ok(Action::Configure(change))
        }
        _ => Err(invalid("Unsupported domain registration command")),
    }
}

fn bind_operation(
    resolved: &crate::workspace_config::Resolved,
    client: &im_core::ImClient,
    command: &ParsedCommand,
    change: &mut ConfigureRegistration,
) -> Result<(), ExitError> {
    let mut semantic = serialize(&*change)?;
    semantic
        .as_object_mut()
        .ok_or_else(|| invalid("Invalid operation"))?
        .remove("operation_id");
    let identity = serialize(client.current_identity())?;
    let actor = identity
        .get("handle")
        .filter(|value| !value.is_null())
        .cloned()
        .unwrap_or_else(|| json!(client.did()));
    let scope = json!({"backend": resolved.service_base_url, "user_service": resolved.user_service_endpoint,
                      "actor": actor, "request": semantic});
    let bytes = serde_json::to_vec(&canonical_value(&scope))
        .map_err(|_| invalid("Cannot encode operation identity"))?;
    let request_hash = format!("{:x}", Sha256::digest(bytes));
    let directory = Path::new(&resolved.paths.state_dir).join("tenant-registration-operations");
    private_directory(&directory)?;
    let explicit = flag(command, "operation-id");
    let filename = explicit
        .as_ref()
        .map(|id| format!("operation-{id}.json"))
        .unwrap_or_else(|| format!("request-{request_hash}.json"));
    let path = directory.join(filename);
    if path.exists() {
        change.operation_id = existing_operation(&path, &request_hash, explicit.as_deref())?;
        return Ok(());
    }
    let operation = explicit.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let receipt = json!({"version":1,"operation_id":operation,"request_hash":request_hash});
    let bytes =
        serde_json::to_vec(&receipt).map_err(|_| invalid("Cannot persist operation identity"))?;
    // A racing create or a durability error must stop before the network. A
    // subsequent invocation inspects and synchronizes the original record.
    private_create(&path, &bytes)?;
    change.operation_id = operation;
    Ok(())
}

fn existing_operation(
    path: &Path,
    hash: &str,
    explicit: Option<&str>,
) -> Result<String, ExitError> {
    let value: Value = serde_json::from_slice(&private_read(path, 4096)?)
        .map_err(|_| invalid("Invalid private operation record"))?;
    let id = value["operation_id"]
        .as_str()
        .ok_or_else(|| invalid("Missing operation id"))?;
    let uuid = uuid::Uuid::parse_str(id).map_err(|_| invalid("Invalid persisted operation id"))?;
    if value["version"] != 1
        || value["request_hash"] != hash
        || uuid.is_nil()
        || uuid.to_string() != id
        || explicit.is_some_and(|value| value != id)
    {
        return Err(invalid(
            "Operation record belongs to another request; inspect it before retrying",
        ));
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .and_then(|file| file.sync_all())
        .map_err(|_| invalid("Cannot synchronize the existing operation record"))?;
    #[cfg(unix)]
    {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| invalid("Cannot synchronize the operation directory"))?;
    }
    Ok(id.to_owned())
}

fn canonical_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            Value::Object(
                keys.into_iter()
                    .map(|key| (key.clone(), canonical_value(&object[key])))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical_value).collect()),
        value => value.clone(),
    }
}

fn append_export(
    contacts: &mut Vec<AllowlistContact>,
    revision: &mut Option<i64>,
    page: &im_core::tenant_registration::AllowlistPage,
) -> Result<(), ExitError> {
    if revision.is_some_and(|value| value != page.revision) {
        return Err(invalid(
            "Policy changed during export; no output file was created",
        ));
    }
    *revision = Some(page.revision);
    for entry in &page.entries {
        contacts.push(AllowlistContact {
            contact_type: entry.contact_type,
            value: entry
                .value
                .clone()
                .ok_or_else(|| invalid("Export response omitted a contact value"))?,
        });
    }
    if contacts.len() > 10_000 {
        return Err(invalid("Export exceeded the domain entry limit"));
    }
    Ok(())
}

fn finish_export(
    domain: &SiteDomain,
    path: &str,
    revision: Option<i64>,
    contacts: &[AllowlistContact],
) -> Result<Value, ExitError> {
    let bytes =
        serde_json::to_vec_pretty(contacts).map_err(|_| invalid("Cannot encode private export"))?;
    private_create(Path::new(path), &bytes)?;
    Ok(json!({"domain":domain,"revision":revision,"entry_count":contacts.len(),"output":path}))
}

fn private_read(path: &Path, max: u64) -> Result<Vec<u8>, ExitError> {
    let before = fs::symlink_metadata(path).map_err(|_| invalid("Private file is unavailable"))?;
    if !before.is_file() || before.file_type().is_symlink() || before.len() > max {
        return Err(invalid("Expected a bounded regular private file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if before.permissions().mode() & 0o077 != 0 {
            return Err(invalid("Private files must have mode 0600"));
        }
    }
    let mut file = fs::File::open(path).map_err(|_| invalid("Cannot open private file"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let after = file
            .metadata()
            .map_err(|_| invalid("Cannot inspect private file"))?;
        if before.dev() != after.dev() || before.ino() != after.ino() {
            return Err(invalid("Private file changed while opening"));
        }
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid("Cannot read private file"))?;
    if bytes.len() as u64 > max {
        return Err(invalid("Private file exceeded its size limit"));
    }
    Ok(bytes)
}

fn private_directory(path: &Path) -> Result<(), ExitError> {
    if !path.exists() {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(path)
            .map_err(|_| invalid("Cannot create private operation directory"))?;
    }
    let meta =
        fs::symlink_metadata(path).map_err(|_| invalid("Cannot inspect operation directory"))?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(invalid("Operation directory must be a regular directory"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err(invalid("Operation directory must have mode 0700"));
        }
    }
    Ok(())
}

fn private_create(path: &Path, bytes: &[u8]) -> Result<(), ExitError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|_| invalid("Private output exists or cannot be created"))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| invalid("Private output is incomplete; inspect before retrying"))?;
    #[cfg(unix)]
    {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| invalid("Cannot synchronize private output directory"))?;
    }
    Ok(())
}

fn dry_plan(action: &Action) -> Value {
    match action {
        Action::Show(domain) | Action::Operation(domain, _) => {
            json!({"plan":{"domain":domain,"action":"read"}})
        }
        Action::List(query) => {
            json!({"plan":{"domain":query.domain,"action":"list","include_values":false}})
        }
        Action::Export(domain, _) => json!({"plan":{"domain":domain,"action":"private_export"}}),
        Action::Configure(request) => json!({"plan":{"domain":request.domain,"mode":request.mode,
            "expected_revision":request.expected_revision,"added_count":request.add_entries.len(),"removed_count":request.remove_entry_ids.len()}}),
    }
}

fn flag(command: &ParsedCommand, name: &str) -> Option<String> {
    command
        .flags
        .get(name)
        .filter(|value| !value.is_empty())
        .cloned()
}
fn required(command: &ParsedCommand, name: &str) -> Result<String, ExitError> {
    flag(command, name).ok_or_else(|| invalid(&format!("--{name} is required")))
}
fn contact_type(value: &str) -> Result<ContactType, ExitError> {
    match value {
        "phone" => Ok(ContactType::Phone),
        "email" => Ok(ContactType::Email),
        _ => Err(invalid("Method must be phone or email")),
    }
}
fn serialize(value: impl serde::Serialize) -> Result<Value, ExitError> {
    serde_json::to_value(value).map_err(|_| invalid("Cannot encode domain registration result"))
}
fn invalid(message: &str) -> ExitError {
    ExitError::new(
        "invalid_argument",
        2,
        message,
        "Check the domain, private input file and current policy revision.",
    )
}
fn map_error(error: im_core::ImError) -> ExitError {
    crate::m_core_cli_adapter::map_im_error(error, "domain registration policy")
}

#[cfg(test)]
#[path = "tenant_registration_tests.rs"]
mod tests;
