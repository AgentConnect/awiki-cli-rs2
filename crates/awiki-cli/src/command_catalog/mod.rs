use serde::Serialize;

#[derive(Debug, Clone)]
pub struct FlagSpec {
    pub name: &'static str,
    pub flag_type: &'static str,
    pub usage: &'static str,
    pub default: &'static str,
    pub required: bool,
    pub choices: &'static [&'static str],
    pub deprecated: bool,
}

#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub name: &'static str,
    pub use_: &'static str,
    pub short: &'static str,
    pub long: &'static str,
    pub aliases: &'static [&'static str],
    pub phase: &'static str,
    pub hidden: bool,
    pub implemented: bool,
    pub handler: &'static str,
    pub side_effect: bool,
    pub outputs: &'static [&'static str],
    pub flags: &'static [FlagSpec],
}

#[derive(Debug, Clone, Copy)]
pub struct CommandSchemaSpec<'a> {
    spec: &'a CommandSpec,
    include_deprecated_flags: bool,
}

impl<'a> CommandSchemaSpec<'a> {
    pub fn default_surface(spec: &'a CommandSpec) -> Self {
        Self {
            spec,
            include_deprecated_flags: false,
        }
    }
}

#[derive(Debug, Clone)]
pub enum SchemaSpecList<'a> {
    All(Vec<CommandSpec>),
    Default(Vec<CommandSchemaSpec<'a>>),
}

#[derive(Debug, Clone, Copy)]
pub enum SchemaCommandSpec<'a> {
    All(&'a CommandSpec),
    Default(CommandSchemaSpec<'a>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandAudience {
    DefaultUser,
    AdvancedUser,
    Operator,
    Diagnostic,
    MigrationOnly,
    InternalService,
}

impl CommandAudience {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DefaultUser => "default",
            Self::AdvancedUser => "advanced",
            Self::Operator => "operator",
            Self::Diagnostic => "diagnostic",
            Self::MigrationOnly => "migration",
            Self::InternalService => "internal",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandOwner {
    CliShell,
    ImCoreIdentity,
    ImCoreAuth,
    ImCoreDirectory,
    ImCoreMessages,
    ImCoreGroups,
    ImCoreAttachments,
    ImCoreRealtime,
    ImCoreSecure,
    ImCoreEmail,
    ImCoreContent,
    ImCoreSite,
    CliDiagnostic,
    CliMigration,
    ExternalUnsupported,
}

impl CommandOwner {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CliShell => "cli_shell",
            Self::ImCoreIdentity => "im_core_identity",
            Self::ImCoreAuth => "im_core_auth",
            Self::ImCoreDirectory => "im_core_directory",
            Self::ImCoreMessages => "im_core_messages",
            Self::ImCoreGroups => "im_core_groups",
            Self::ImCoreAttachments => "im_core_attachments",
            Self::ImCoreRealtime => "im_core_realtime",
            Self::ImCoreSecure => "im_core_secure",
            Self::ImCoreEmail => "im_core_email",
            Self::ImCoreContent => "im_core_content",
            Self::ImCoreSite => "im_core_site",
            Self::CliDiagnostic => "cli_diagnostic",
            Self::CliMigration => "cli_migration",
            Self::ExternalUnsupported => "external_unsupported",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliShellRole {
    None,
    ParsesInputOnly,
    WritesDefaultIdentityFile,
    ReadsUserInputFile,
    WritesUserOutputFile,
    RendersDryRunPlan,
    ManagesLocalService,
    ManagesHostNotifyConfig,
    ManagesTenantConfig,
}

impl CliShellRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ParsesInputOnly => "parses_input_only",
            Self::WritesDefaultIdentityFile => "writes_default_identity_file",
            Self::ReadsUserInputFile => "reads_user_input_file",
            Self::WritesUserOutputFile => "writes_user_output_file",
            Self::RendersDryRunPlan => "renders_dry_run_plan",
            Self::ManagesLocalService => "manages_local_service",
            Self::ManagesHostNotifyConfig => "manages_host_notify_config",
            Self::ManagesTenantConfig => "manages_tenant_config",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectInvocationPolicy {
    Allow,
    AllowWithWarning,
    RequireDiagnosticGate,
    RequireMigrationGate,
    RequireInternalServiceGate,
    StableUnsupported {
        capability: &'static str,
        phase: &'static str,
    },
    Removed {
        replacement: Option<&'static str>,
    },
    DeprecatedAlias {
        replacement: &'static str,
        until: &'static str,
    },
}

impl DirectInvocationPolicy {
    pub fn kind(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::AllowWithWarning => "allow_with_warning",
            Self::RequireDiagnosticGate => "require_diagnostic_gate",
            Self::RequireMigrationGate => "require_migration_gate",
            Self::RequireInternalServiceGate => "require_internal_service_gate",
            Self::StableUnsupported { .. } => "stable_unsupported",
            Self::Removed { .. } => "removed",
            Self::DeprecatedAlias { .. } => "deprecated_alias",
        }
    }

    pub fn is_default_invocable(self) -> bool {
        matches!(self, Self::Allow | Self::AllowWithWarning)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutoverStatus {
    CliOwned,
    ImCore,
    Unsupported {
        capability: &'static str,
        phase: &'static str,
    },
    Hidden,
    Removed,
    DiagnosticOnly,
}

impl CutoverStatus {
    pub fn kind(self) -> &'static str {
        match self {
            Self::CliOwned => "cli_owned",
            Self::ImCore => "im_core",
            Self::Unsupported { .. } => "unsupported",
            Self::Hidden => "hidden",
            Self::Removed => "removed",
            Self::DiagnosticOnly => "diagnostic_only",
        }
    }

    pub fn capability(self) -> Option<&'static str> {
        match self {
            Self::Unsupported { capability, .. } => Some(capability),
            _ => None,
        }
    }

    pub fn required_phase(self) -> Option<&'static str> {
        match self {
            Self::Unsupported { phase, .. } => Some(phase),
            _ => None,
        }
    }

    pub fn include_in_default_surface(self) -> bool {
        matches!(self, Self::CliOwned | Self::ImCore)
    }
}

#[derive(Debug, Clone, Copy)]
struct CommandCutoverView<'a> {
    status: CutoverStatus,
    default_surface: bool,
    capability: Option<&'a str>,
    required_phase: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCommand {
    pub name: String,
    pub consumed_words: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandResolveError {
    MissingCommand,
    UnknownSubcommand {
        parent: &'static str,
        subcommand: String,
    },
}

impl CommandSpec {
    pub fn json_use(&self) -> &'static str {
        self.use_
    }

    pub fn canonical_name(&self) -> &'static str {
        self.name
    }

    pub fn cutover_status(&self) -> CutoverStatus {
        cutover_status(self.name)
    }

    pub fn audience(&self) -> CommandAudience {
        command_audience(self.name)
    }

    pub fn primary_owner(&self) -> CommandOwner {
        primary_owner(self.name)
    }

    pub fn secondary_owners(&self) -> &'static [CommandOwner] {
        secondary_owners(self.name)
    }

    pub fn cli_shell_role(&self) -> CliShellRole {
        cli_shell_role(self.name)
    }

    pub fn direct_invocation(&self) -> DirectInvocationPolicy {
        direct_invocation_policy(self.name)
    }

    pub fn include_in_default_surface(&self) -> bool {
        self.audience() == CommandAudience::DefaultUser
            && default_surface_owner(self.primary_owner())
            && self.direct_invocation().is_default_invocable()
    }
}

pub fn specs() -> Vec<CommandSpec> {
    default_specs().to_vec()
}

pub fn default_surface_specs() -> Vec<CommandSpec> {
    default_specs()
        .iter()
        .filter(|spec| spec.include_in_default_surface())
        .cloned()
        .collect()
}

pub fn public_help_root_specs() -> Vec<CommandSpec> {
    default_specs()
        .iter()
        .filter(|spec| parent_name(spec.name).is_empty())
        .filter(|spec| include_in_public_help(spec))
        .cloned()
        .collect()
}

pub fn default_surface_schema_specs() -> Vec<CommandSchemaSpec<'static>> {
    default_specs()
        .iter()
        .filter(|spec| spec.include_in_default_surface())
        .map(CommandSchemaSpec::default_surface)
        .collect()
}

pub fn audience_surface_specs(raw: &str) -> Option<Vec<CommandSpec>> {
    let audience = match raw.trim().to_ascii_lowercase().as_str() {
        "default" => return Some(default_surface_specs()),
        "advanced" => CommandAudience::AdvancedUser,
        "operator" => CommandAudience::Operator,
        "diagnostic" => CommandAudience::Diagnostic,
        "migration" => CommandAudience::MigrationOnly,
        "internal" => CommandAudience::InternalService,
        "all" => return Some(specs()),
        _ => return None,
    };
    Some(
        default_specs()
            .iter()
            .filter(|spec| spec.audience() == audience)
            .cloned()
            .collect(),
    )
}

pub fn audience_schema_specs(raw: &str) -> Option<SchemaSpecList<'static>> {
    let audience = match raw.trim().to_ascii_lowercase().as_str() {
        "default" => {
            return Some(SchemaSpecList::Default(default_surface_schema_specs()));
        }
        "advanced" => CommandAudience::AdvancedUser,
        "operator" => CommandAudience::Operator,
        "diagnostic" => CommandAudience::Diagnostic,
        "migration" => CommandAudience::MigrationOnly,
        "internal" => CommandAudience::InternalService,
        "all" => return Some(SchemaSpecList::All(specs())),
        _ => return None,
    };
    Some(SchemaSpecList::All(
        default_specs()
            .iter()
            .filter(|spec| spec.audience() == audience)
            .cloned()
            .collect(),
    ))
}

pub fn schema_spec_for_command(spec: &CommandSpec) -> SchemaCommandSpec<'_> {
    if spec.include_in_default_surface() {
        SchemaCommandSpec::Default(CommandSchemaSpec::default_surface(spec))
    } else {
        SchemaCommandSpec::All(spec)
    }
}

pub fn lookup(raw: &str) -> Option<CommandSpec> {
    let needle = normalize_name(raw);
    default_specs()
        .iter()
        .find(|spec| normalize_name(spec.name) == needle)
        .cloned()
}

pub fn cutover_status(raw: &str) -> CutoverStatus {
    try_cutover_status(raw).unwrap_or(CutoverStatus::Removed)
}

pub fn try_cutover_status(raw: &str) -> Option<CutoverStatus> {
    let name = normalize_name(raw);
    let name = name.as_str();
    if is_one_of(
        name,
        &[
            "id.create",
            "runtime.listener.run",
            "runtime.listener.service-run",
            "runtime.host-notify.hermes.bridge",
            "runtime.host-notify.hermes.bridge.service-run",
            "debug.schema-cache",
            "debug.logs",
        ],
    ) {
        return Some(CutoverStatus::Hidden);
    }
    if has_any_command_prefix(name, &["group.code", "debug.raw"]) {
        return Some(CutoverStatus::Removed);
    }
    if has_any_command_prefix(name, &["runtime.host-notify.openclaw"])
        || is_one_of(
            name,
            &[
                "id.import-v1",
                "id.replace-did",
                "runtime.host-notify.hermes.set",
                "runtime.host-notify.hermes.set-secret",
                "runtime.host-notify.hermes.clear-secret",
                "debug",
                "debug.db",
                "debug.db.handle-history",
                "debug.db.import-v1",
            ],
        )
    {
        return Some(CutoverStatus::DiagnosticOnly);
    }
    if has_command_prefix(name, "group.e2ee")
        && !is_one_of(name, &["group.e2ee.status", "group.e2ee.repair"])
    {
        return Some(CutoverStatus::DiagnosticOnly);
    }
    if name == "debug.db.query" {
        return Some(CutoverStatus::Unsupported {
            capability: "raw-sql",
            phase: "outside current im-core cutover",
        });
    }
    if is_one_of(
        name,
        &[
            "msg.secure.init",
            "msg.secure.failed",
            "msg.secure.retry",
            "msg.secure.drop",
        ],
    ) || has_command_prefix(name, "msg.secure.outbox")
    {
        return Some(CutoverStatus::Unsupported {
            capability: "secure-direct",
            phase: "Phase 6",
        });
    }
    if name == "group.secure.diagnostics" {
        return Some(CutoverStatus::Unsupported {
            capability: "group secure diagnostics",
            phase: "future diagnostics plan",
        });
    }
    if has_command_prefix(name, "runtime.heartbeat") {
        return Some(CutoverStatus::Unsupported {
            capability: "runtime-heartbeat",
            phase: "outside current im-core cutover",
        });
    }
    if name == "people.search" {
        return Some(CutoverStatus::Unsupported {
            capability: "people-directory",
            phase: "future people search API",
        });
    }
    if has_command_prefix(name, "id.device") {
        return Some(CutoverStatus::ImCore);
    }
    if is_one_of(
        name,
        &[
            "id",
            "id.status",
            "id.vault",
            "id.vault.status",
            "id.vault.migrate",
            "id.vault.cleanup-plaintext",
            "id.register",
            "id.bind",
            "id.refresh-token",
            "id.resolve",
            "id.recover",
            "id.list",
            "id.current",
            "id.use",
            "id.profile",
            "id.profile.get",
            "id.profile.set",
            "msg",
            "msg.attachment",
            "msg.attachment.download",
            "msg.send",
            "msg.inbox",
            "msg.history",
            "msg.mark-read",
            "msg.secure",
            "msg.secure.status",
            "msg.secure.repair",
            "mail",
            "mail.account",
            "mail.attachment",
            "mail.attachment.download",
            "mail.inbox",
            "mail.mark-read",
            "mail.notify",
            "mail.read",
            "mail.send",
            "group",
            "group.create",
            "group.get",
            "group.join",
            "group.add",
            "group.remove",
            "group.leave",
            "group.update",
            "group.list",
            "group.members",
            "group.messages",
            "group.secure",
            "group.secure.status",
            "group.secure.repair",
            "group.e2ee.status",
            "group.e2ee.repair",
            "people",
            "people.follow",
            "people.unfollow",
            "people.status",
            "people.followers",
            "people.following",
            "people.contacts",
            "people.contacts.list",
            "people.contacts.save",
            "page",
            "page.create",
            "page.list",
            "page.get",
            "page.update",
            "page.rename",
            "page.delete",
            "site",
            "site.root",
            "site.root.get",
            "site.root.set",
            "site.page",
            "site.page.list",
            "site.page.get",
            "site.page.create",
            "site.page.update",
            "site.page.rename",
            "site.page.delete",
        ],
    ) {
        return Some(CutoverStatus::ImCore);
    }
    if is_one_of(
        name,
        &[
            "status",
            "help",
            "docs",
            "schema",
            "doctor",
            "version",
            "upgrade",
            "init",
            "completion",
            "completion.bash",
            "completion.zsh",
            "completion.fish",
            "completion.powershell",
            "config",
            "config.show",
            "tenant",
            "tenant.list",
            "tenant.current",
            "tenant.create",
            "tenant.setup",
            "tenant.use",
            "tenant.reconfigure",
            "runtime",
            "runtime.status",
            "runtime.apply",
            "runtime.setup",
            "runtime.mode",
            "runtime.mode.get",
            "runtime.mode.set",
            "runtime.listener",
            "runtime.listener.status",
            "runtime.listener.install",
            "runtime.listener.start",
            "runtime.listener.stop",
            "runtime.listener.restart",
            "runtime.listener.uninstall",
            "runtime.listener.config",
            "runtime.listener.config.show",
            "runtime.listener.config.set",
            "runtime.listener.enable",
            "runtime.listener.disable",
            "runtime.host-notify",
            "runtime.host-notify.config",
            "runtime.host-notify.config.show",
            "runtime.host-notify.config.set",
            "runtime.host-notify.enable",
            "runtime.host-notify.disable",
            "runtime.host-notify.hermes",
            "runtime.host-notify.hermes.guide",
            "runtime.host-notify.hermes.status",
            "runtime.host-notify.hermes.setup",
        ],
    ) {
        return Some(CutoverStatus::CliOwned);
    }
    None
}

pub fn command_audience(raw: &str) -> CommandAudience {
    let name = normalize_name(raw);
    let name = name.as_str();
    if has_command_prefix(name, "id.device") {
        return CommandAudience::AdvancedUser;
    }
    if is_one_of(
        name,
        &[
            "runtime.listener.run",
            "runtime.listener.service-run",
            "runtime.host-notify.hermes.bridge",
            "runtime.host-notify.hermes.bridge.service-run",
        ],
    ) || has_any_command_prefix(name, &["group.code", "debug.raw"])
    {
        return CommandAudience::InternalService;
    }
    if has_command_prefix(name, "group.e2ee")
        && !is_one_of(name, &["group.e2ee.status", "group.e2ee.repair"])
    {
        return CommandAudience::InternalService;
    }
    if is_one_of(
        name,
        &[
            "id.create",
            "id.import-v1",
            "id.vault.migrate",
            "id.vault.cleanup-plaintext",
            "debug.db.import-v1",
        ],
    ) {
        return CommandAudience::MigrationOnly;
    }
    if is_one_of(
        name,
        &[
            "id.replace-did",
            "debug",
            "debug.db",
            "debug.db.handle-history",
            "debug.db.query",
            "debug.schema-cache",
            "debug.logs",
        ],
    ) {
        return CommandAudience::Diagnostic;
    }
    if is_one_of(
        name,
        &[
            "msg.secure.init",
            "msg.secure.failed",
            "msg.secure.retry",
            "msg.secure.drop",
            "group.secure.diagnostics",
        ],
    ) || has_command_prefix(name, "msg.secure.outbox")
    {
        return CommandAudience::AdvancedUser;
    }
    if is_one_of(
        name,
        &[
            "runtime.host-notify.hermes.set",
            "runtime.host-notify.hermes.set-secret",
            "runtime.host-notify.hermes.clear-secret",
        ],
    ) {
        return CommandAudience::Diagnostic;
    }
    if has_command_prefix(name, "tenant") {
        return CommandAudience::AdvancedUser;
    }
    if has_any_command_prefix(name, &["runtime.host-notify.openclaw"]) {
        return CommandAudience::Operator;
    }
    if has_command_prefix(name, "runtime.host-notify.hermes") {
        return CommandAudience::Operator;
    }
    if has_any_command_prefix(name, &["runtime.setup", "runtime.apply"]) {
        return CommandAudience::Operator;
    }
    if is_one_of(
        name,
        &[
            "runtime.listener.install",
            "runtime.listener.start",
            "runtime.listener.stop",
            "runtime.listener.restart",
            "runtime.listener.uninstall",
            "runtime.host-notify.hermes.guide",
            "runtime.host-notify.hermes.status",
            "runtime.host-notify.hermes.setup",
        ],
    ) {
        return CommandAudience::Operator;
    }
    if has_any_command_prefix(
        name,
        &[
            "runtime.mode",
            "runtime.listener.config",
            "runtime.host-notify.config",
            "runtime.heartbeat",
        ],
    ) {
        return CommandAudience::AdvancedUser;
    }
    CommandAudience::DefaultUser
}

pub fn primary_owner(raw: &str) -> CommandOwner {
    let name = normalize_name(raw);
    let name = name.as_str();
    if has_any_command_prefix(name, &["debug.raw", "group.code"]) {
        return CommandOwner::ExternalUnsupported;
    }
    if has_command_prefix(name, "runtime.heartbeat") || name == "people.search" {
        return CommandOwner::ExternalUnsupported;
    }
    if has_command_prefix(name, "debug") {
        return CommandOwner::CliDiagnostic;
    }
    if is_one_of(
        name,
        &[
            "id.create",
            "id.import-v1",
            "id.vault.migrate",
            "id.vault.cleanup-plaintext",
        ],
    ) {
        return CommandOwner::CliMigration;
    }
    if name == "id.refresh-token" {
        return CommandOwner::ImCoreAuth;
    }
    if name == "id.resolve" {
        return CommandOwner::ImCoreDirectory;
    }
    if has_command_prefix(name, "id") {
        return CommandOwner::ImCoreIdentity;
    }
    if name == "msg.attachment" || name == "msg.attachment.download" {
        return CommandOwner::ImCoreAttachments;
    }
    if has_command_prefix(name, "msg.secure") || has_command_prefix(name, "group.secure") {
        return CommandOwner::ImCoreSecure;
    }
    if has_command_prefix(name, "msg") {
        return CommandOwner::ImCoreMessages;
    }
    if has_command_prefix(name, "mail") {
        return CommandOwner::ImCoreEmail;
    }
    if has_command_prefix(name, "group.e2ee") {
        return CommandOwner::ImCoreSecure;
    }
    if has_command_prefix(name, "group") {
        return CommandOwner::ImCoreGroups;
    }
    if has_command_prefix(name, "people") {
        return CommandOwner::ImCoreDirectory;
    }
    if has_command_prefix(name, "page") {
        return CommandOwner::ImCoreContent;
    }
    if has_command_prefix(name, "site") {
        return CommandOwner::ImCoreSite;
    }
    if has_command_prefix(name, "runtime.listener.run")
        || has_command_prefix(name, "runtime.listener.service-run")
    {
        return CommandOwner::ImCoreRealtime;
    }
    CommandOwner::CliShell
}

pub fn secondary_owners(raw: &str) -> &'static [CommandOwner] {
    let name = normalize_name(raw);
    let name = name.as_str();
    if name == "id.status" {
        return &[CommandOwner::ImCoreAuth];
    }
    if name == "id.bind" {
        return &[CommandOwner::ImCoreDirectory];
    }
    if name == "msg.send" || name == "msg.attachment" || name == "msg.attachment.download" {
        return &[CommandOwner::ImCoreMessages];
    }
    if has_command_prefix(name, "group.e2ee") || has_command_prefix(name, "group.secure") {
        return &[CommandOwner::ImCoreGroups];
    }
    &[]
}

pub fn cli_shell_role(raw: &str) -> CliShellRole {
    let name = normalize_name(raw);
    let name = name.as_str();
    if name == "id.device.join.approve" {
        return CliShellRole::ParsesInputOnly;
    }
    if name == "id.use" {
        return CliShellRole::WritesDefaultIdentityFile;
    }
    if is_one_of(
        name,
        &[
            "id.register",
            "id.recover",
            "id.vault.migrate",
            "id.vault.cleanup-plaintext",
            "id.replace-did",
            "id.create",
            "id.import-v1",
            "debug.db.import-v1",
            "group.create",
            "group.join",
            "group.add",
            "group.remove",
            "group.leave",
            "group.update",
            "group.e2ee.publish-key-package",
            "group.e2ee.update-key",
            "group.e2ee.rejoin",
            "group.e2ee.recover-member",
            "group.e2ee.process-leave-request",
        ],
    ) {
        return CliShellRole::RendersDryRunPlan;
    }
    if is_one_of(name, &["msg.send", "mail.send"])
        || has_any_command_prefix(name, &["page", "site"])
    {
        return CliShellRole::ReadsUserInputFile;
    }
    if is_one_of(
        name,
        &["msg.attachment.download", "mail.attachment.download"],
    ) {
        return CliShellRole::WritesUserOutputFile;
    }
    if has_command_prefix(name, "runtime.listener") {
        return CliShellRole::ManagesLocalService;
    }
    if has_command_prefix(name, "runtime.host-notify") {
        return CliShellRole::ManagesHostNotifyConfig;
    }
    if has_command_prefix(name, "tenant") {
        return CliShellRole::ManagesTenantConfig;
    }
    CliShellRole::None
}

pub fn direct_invocation_policy(raw: &str) -> DirectInvocationPolicy {
    let name = normalize_name(raw);
    let name = name.as_str();
    if has_command_prefix(name, "debug.raw") {
        return DirectInvocationPolicy::Removed { replacement: None };
    }
    if has_command_prefix(name, "group.code") {
        return DirectInvocationPolicy::Removed {
            replacement: Some("group join"),
        };
    }
    if has_any_command_prefix(
        name,
        &[
            "runtime.listener.run",
            "runtime.listener.service-run",
            "runtime.host-notify.hermes.bridge",
        ],
    ) {
        return DirectInvocationPolicy::RequireInternalServiceGate;
    }
    if is_one_of(
        name,
        &[
            "id.create",
            "id.import-v1",
            "id.vault.migrate",
            "id.vault.cleanup-plaintext",
            "debug.db.import-v1",
        ],
    ) {
        return DirectInvocationPolicy::RequireMigrationGate;
    }
    if is_one_of(
        name,
        &[
            "id.replace-did",
            "debug",
            "debug.db",
            "debug.db.handle-history",
            "debug.schema-cache",
            "debug.logs",
            "runtime.host-notify.hermes.set",
            "runtime.host-notify.hermes.set-secret",
            "runtime.host-notify.hermes.clear-secret",
        ],
    ) {
        return DirectInvocationPolicy::RequireDiagnosticGate;
    }
    if name == "debug.db.query" {
        return DirectInvocationPolicy::StableUnsupported {
            capability: "raw-sql",
            phase: "outside current im-core cutover",
        };
    }
    if is_one_of(
        name,
        &["msg.secure.status", "msg.secure.repair", "msg.secure"],
    ) {
        return DirectInvocationPolicy::Allow;
    }
    if name == "group.secure.diagnostics" {
        return DirectInvocationPolicy::StableUnsupported {
            capability: "group secure diagnostics",
            phase: "future diagnostics plan",
        };
    }
    if has_command_prefix(name, "msg.secure") {
        return DirectInvocationPolicy::StableUnsupported {
            capability: "secure-direct",
            phase: "Phase 6",
        };
    }
    if is_one_of(
        name,
        &["group.secure.status", "group.secure.repair", "group.secure"],
    ) {
        return DirectInvocationPolicy::Allow;
    }
    if name == "group.e2ee.status" {
        return DirectInvocationPolicy::DeprecatedAlias {
            replacement: "group secure status",
            until: "next-major",
        };
    }
    if name == "group.e2ee.repair" {
        return DirectInvocationPolicy::DeprecatedAlias {
            replacement: "group secure repair",
            until: "next-major",
        };
    }
    if has_command_prefix(name, "group.e2ee") {
        return DirectInvocationPolicy::RequireInternalServiceGate;
    }
    if name == "people.search" {
        return DirectInvocationPolicy::StableUnsupported {
            capability: "people-directory",
            phase: "future people search API",
        };
    }
    if has_command_prefix(name, "runtime.heartbeat") {
        return DirectInvocationPolicy::StableUnsupported {
            capability: "runtime-heartbeat",
            phase: "outside current im-core cutover",
        };
    }
    DirectInvocationPolicy::Allow
}

pub fn children_of(parent: &str) -> Vec<CommandSpec> {
    let needle = normalize_name(parent);
    let mut children: Vec<_> = default_specs()
        .iter()
        .filter(|spec| parent_name(spec.name) == needle)
        .cloned()
        .collect();
    children.sort_by_key(|spec| spec.name);
    children
}

pub fn public_help_children_of(parent: &str) -> Vec<CommandSpec> {
    let needle = normalize_name(parent);
    let mut children: Vec<_> = default_specs()
        .iter()
        .filter(|spec| parent_name(spec.name) == needle)
        .filter(|spec| include_in_public_help(spec))
        .cloned()
        .collect();
    children.sort_by_key(|spec| spec.name);
    children
}

pub fn default_surface_schema_children_of(parent: &str) -> Vec<CommandSchemaSpec<'static>> {
    let needle = normalize_name(parent);
    let mut children: Vec<_> = default_specs()
        .iter()
        .filter(|spec| parent_name(spec.name) == needle)
        .filter(|spec| spec.include_in_default_surface())
        .map(CommandSchemaSpec::default_surface)
        .collect();
    children.sort_by_key(|spec| spec.spec.name);
    children
}

pub fn resolve_command(words: &[&str]) -> Result<ResolvedCommand, CommandResolveError> {
    if words.is_empty() {
        return Err(CommandResolveError::MissingCommand);
    }

    let mut best: Option<ResolvedCommand> = None;
    for spec in default_specs() {
        for path in command_paths(spec) {
            if path.len() > words.len() || !path_matches(&path, words) {
                continue;
            }
            let is_better = match best.as_ref() {
                Some(current) => path.len() > current.consumed_words,
                None => true,
            };
            if is_better {
                best = Some(ResolvedCommand {
                    name: spec.name.to_string(),
                    consumed_words: path.len(),
                });
            }
        }
    }

    if let Some(resolved) = best {
        if resolved.name == "group.e2ee" && words.len() > resolved.consumed_words {
            return Err(CommandResolveError::UnknownSubcommand {
                parent: "group e2ee",
                subcommand: words[resolved.consumed_words].to_string(),
            });
        }
        return Ok(resolved);
    }

    Ok(ResolvedCommand {
        name: words[0].to_string(),
        consumed_words: 1,
    })
}

pub fn has_local_flag(command_name: &str, flag_name: &str) -> bool {
    lookup(command_name).is_some_and(|spec| {
        spec.flags
            .iter()
            .any(|flag| flag.name.eq_ignore_ascii_case(flag_name))
    })
}

pub fn is_local_bool_flag(command_name: &str, flag_name: &str) -> bool {
    lookup(command_name).is_some_and(|spec| {
        spec.flags
            .iter()
            .any(|flag| flag.name.eq_ignore_ascii_case(flag_name) && flag.flag_type == "bool")
    })
}

pub fn normalize_name(raw: &str) -> String {
    raw.trim()
        .strip_prefix("awiki-cli")
        .unwrap_or(raw.trim())
        .trim()
        .replace(' ', ".")
        .trim_matches('.')
        .to_ascii_lowercase()
}

fn command_paths(spec: &CommandSpec) -> Vec<Vec<&'static str>> {
    let segments: Vec<_> = spec.name.split('.').collect();
    let mut paths: Vec<Vec<&'static str>> = vec![Vec::new()];
    for index in 0..segments.len() {
        let mut segment_names = vec![segments[index]];
        segment_names.extend(aliases_for_prefix(&segments[..=index]));

        let mut next_paths = Vec::new();
        for path in &paths {
            for segment_name in &segment_names {
                let mut next_path = path.clone();
                next_path.push(*segment_name);
                next_paths.push(next_path);
            }
        }
        paths = next_paths;
    }
    paths
}

fn aliases_for_prefix(prefix_segments: &[&str]) -> &'static [&'static str] {
    default_specs()
        .iter()
        .find(|spec| spec.name.split('.').eq(prefix_segments.iter().copied()))
        .map(|spec| spec.aliases)
        .unwrap_or(&[])
}

fn path_matches(path: &[&str], words: &[&str]) -> bool {
    path.iter()
        .zip(words.iter())
        .all(|(expected, actual)| expected == actual)
}

fn is_one_of(name: &str, values: &[&str]) -> bool {
    values.contains(&name)
}

fn has_command_prefix(name: &str, prefix: &str) -> bool {
    name == prefix
        || name
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with('.'))
}

fn has_any_command_prefix(name: &str, prefixes: &[&str]) -> bool {
    prefixes
        .iter()
        .any(|prefix| has_command_prefix(name, prefix))
}

fn include_in_public_help(spec: &CommandSpec) -> bool {
    if spec.hidden {
        return false;
    }
    matches!(
        spec.direct_invocation(),
        DirectInvocationPolicy::Allow
            | DirectInvocationPolicy::AllowWithWarning
            | DirectInvocationPolicy::RequireDiagnosticGate
            | DirectInvocationPolicy::RequireMigrationGate
            | DirectInvocationPolicy::RequireInternalServiceGate
    )
}

fn default_surface_owner(owner: CommandOwner) -> bool {
    matches!(
        owner,
        CommandOwner::CliShell
            | CommandOwner::ImCoreIdentity
            | CommandOwner::ImCoreAuth
            | CommandOwner::ImCoreDirectory
            | CommandOwner::ImCoreMessages
            | CommandOwner::ImCoreGroups
            | CommandOwner::ImCoreAttachments
            | CommandOwner::ImCoreRealtime
            | CommandOwner::ImCoreSecure
            | CommandOwner::ImCoreEmail
            | CommandOwner::ImCoreContent
            | CommandOwner::ImCoreSite
    )
}

fn parent_name(name: &str) -> String {
    let normalized = normalize_name(name);
    normalized
        .rfind('.')
        .map(|index| normalized[..index].to_string())
        .unwrap_or_default()
}

macro_rules! flag {
    ($name:expr, $ty:expr, $usage:expr) => {
        FlagSpec {
            name: $name,
            flag_type: $ty,
            usage: $usage,
            default: "",
            required: false,
            choices: &[],
            deprecated: false,
        }
    };
    ($name:expr, $ty:expr, $usage:expr, default = $default:expr) => {
        FlagSpec {
            name: $name,
            flag_type: $ty,
            usage: $usage,
            default: $default,
            required: false,
            choices: &[],
            deprecated: false,
        }
    };
    ($name:expr, $ty:expr, $usage:expr, choices = [$($choice:expr),+ $(,)?]) => {
        FlagSpec {
            name: $name,
            flag_type: $ty,
            usage: $usage,
            default: "",
            required: false,
            choices: &[$($choice),+],
            deprecated: false,
        }
    };
    ($name:expr, $ty:expr, $usage:expr, default = $default:expr, choices = [$($choice:expr),+ $(,)?]) => {
        FlagSpec {
            name: $name,
            flag_type: $ty,
            usage: $usage,
            default: $default,
            required: false,
            choices: &[$($choice),+],
            deprecated: false,
        }
    };
    ($name:expr, $ty:expr, $usage:expr, required) => {
        FlagSpec {
            name: $name,
            flag_type: $ty,
            usage: $usage,
            default: "",
            required: true,
            choices: &[],
            deprecated: false,
        }
    };
    ($name:expr, $ty:expr, $usage:expr, required, choices = [$($choice:expr),+ $(,)?]) => {
        FlagSpec {
            name: $name,
            flag_type: $ty,
            usage: $usage,
            default: "",
            required: true,
            choices: &[$($choice),+],
            deprecated: false,
        }
    };
    ($name:expr, $ty:expr, $usage:expr, deprecated) => {
        FlagSpec {
            name: $name,
            flag_type: $ty,
            usage: $usage,
            default: "",
            required: false,
            choices: &[],
            deprecated: true,
        }
    };
    ($name:expr, $ty:expr, $usage:expr, default = $default:expr, choices = [$($choice:expr),+ $(,)?], deprecated) => {
        FlagSpec {
            name: $name,
            flag_type: $ty,
            usage: $usage,
            default: $default,
            required: false,
            choices: &[$($choice),+],
            deprecated: true,
        }
    };
}

macro_rules! cmd {
    ($name:expr, $use:expr, $short:expr, $phase:expr, $handler:expr) => {
        CommandSpec {
            name: $name,
            use_: $use,
            short: $short,
            long: "",
            aliases: &[],
            phase: $phase,
            hidden: false,
            implemented: true,
            handler: $handler,
            side_effect: false,
            outputs: &["json", "pretty", "table"],
            flags: &[],
        }
    };
}

fn default_specs() -> &'static [CommandSpec] {
    &[
        cmd!("status", "status", "Show the current phase-1 CLI status", "phase1", "status"),
        cmd!("docs", "docs [topic]", "Show built-in documentation topics", "phase1", "docs"),
        cmd!("doctor", "doctor", "Run baseline environment and storage diagnostics", "phase1", "doctor"),
        cmd!("version", "version", "Show build information", "phase1", "version"),
        CommandSpec { name: "upgrade", use_: "upgrade", short: "Check for newer awiki-cli versions and show upgrade hints", long: "", aliases: &[], phase: "phase2", hidden: false, implemented: true, handler: "upgrade", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[] },
        CommandSpec { name: "init", use_: "init", short: "Initialize the awiki-cli workspace and config.yaml", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "init", side_effect: true, outputs: &["json", "pretty", "table"], flags: &[] },
        CommandSpec { name: "completion", use_: "completion", short: "Generate shell completion scripts", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "completion.bash", use_: "bash", short: "Generate Bash completion", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "completion.bash", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "completion.zsh", use_: "zsh", short: "Generate Zsh completion", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "completion.zsh", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "completion.fish", use_: "fish", short: "Generate Fish completion", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "completion.fish", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "completion.powershell", use_: "powershell", short: "Generate PowerShell completion", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "completion.powershell", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "help", use_: "help [COMMAND]", short: "Show human-readable command help", long: "Show concise command usage, flags, and subcommands. For machine-readable command metadata, use `awiki-cli schema`.", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "help", side_effect: false, outputs: &["text"], flags: &[] },
        CommandSpec { name: "schema", use_: "schema [COMMAND]", short: "Show static command contracts", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "schema", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("all", "bool", "Show every command surface"), flag!("audience", "string", "Show one command audience", choices = ["default", "advanced", "operator", "diagnostic", "migration", "internal", "all"])] },
        CommandSpec { name: "config", use_: "config", short: "Inspect resolved CLI configuration", long: "Inspect the configuration resolved from the active tenant and its isolated workspace. Backend and DID host values are managed as an atomic tenant profile, not by editing config.yaml or using the removed config set command.", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        cmd!("config.show", "show", "Show resolved configuration values", "phase1", "config.show"),
        CommandSpec { name: "tenant", use_: "tenant", short: "Manage backend and DID host tenants", long: "A tenant is an atomic backend_base_url + did_host profile with an isolated local workspace under ~/.awiki-cli/tenants/<name>. Create a tenant first, then switch by name; backend and DID host values are not edited in config.yaml.", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "tenant.list", use_: "list", short: "List configured tenants", long: "List tenant profiles from the product-level tenant registry and show the active tenant for this command invocation.", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "tenant.list", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[] },
        CommandSpec { name: "tenant.current", use_: "current", short: "Show the current tenant", long: "Show the tenant selected by global config or by this command's --tenant override.", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "tenant.current", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[] },
        CommandSpec { name: "tenant.create", use_: "create <name>", short: "Create a tenant from backend and DID host", long: "Create a named tenant profile. Tenant names are normalized to lowercase and may contain only ASCII letters, numbers, and single '-' separators; use --display-name for spaces, uppercase presentation, or non-ASCII labels. This writes the tenant registry and initializes the tenant-local config directory, but does not make the new tenant active; run `awiki-cli tenant use <name>` to switch.", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "tenant.create", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("backend-base-url", "string", "Backend base URL for User-Service and Message-Service", required), flag!("did-host", "string", "Bare DID host for handles and DID service discovery", required), flag!("display-name", "string", "Human-readable tenant name")] },
        CommandSpec { name: "tenant.setup", use_: "setup <name>", short: "Idempotently configure and activate a tenant", long: "Create and activate a tenant when it does not exist, or activate an existing tenant only when its normalized backend_base_url and did_host exactly match. This command never reconfigures existing tenant data and does not initialize the tenant workspace; run `awiki-cli init` next.", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "tenant.setup", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("backend-base-url", "string", "Backend base URL for User-Service and Message-Service", required), flag!("did-host", "string", "Bare DID host for handles and DID service discovery", required), flag!("display-name", "string", "Human-readable tenant name")] },
        CommandSpec { name: "tenant.use", use_: "use <name>", short: "Switch the active tenant", long: "Switch the product-level active tenant by name. The tenant must already exist; this command intentionally does not accept backend or DID host fields.", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "tenant.use", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "tenant.reconfigure", use_: "reconfigure <name>", short: "Update an empty tenant's backend and DID host", long: "Update backend_base_url and did_host only for an empty tenant. If the tenant already has identities or local database data, create a new tenant instead.", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "tenant.reconfigure", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("backend-base-url", "string", "New backend base URL", required), flag!("did-host", "string", "New bare DID host", required)] },
        CommandSpec { name: "id", use_: "id", short: "Identity lifecycle commands", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "id.status", use_: "status", short: "Show identity status", long: "", aliases: &[], phase: "phase2", hidden: false, implemented: true, handler: "id.status", side_effect: false, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "id.vault", use_: "vault", short: "Inspect or migrate identity secret vault state", long: "", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "id.vault.status", use_: "status", short: "Show identity SecretVault status", long: "Show the selected identity's SecretVault open options, root-key availability, selected backend, migration metadata status, and plaintext compatibility retention. This command never prints root key material, JWTs, private PEM, or full SecretRef values.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.vault.status", side_effect: false, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "id.vault.migrate", use_: "migrate", short: "Preflight identity migration into SecretVault", long: "Migration-gated SecretVault preflight. In this build, im-core exposes vault-backed register/recover and status but not a CLI-safe standalone migration API, so live execution fails without rewriting identity files.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.vault.migrate", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "id.vault.cleanup-plaintext", use_: "cleanup-plaintext", short: "Preflight plaintext compatibility cleanup", long: "Migration-gated SecretVault cleanup preflight. In this build, im-core exposes status but not a CLI-safe standalone plaintext cleanup API, so live execution fails without deleting identity files.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.vault.cleanup-plaintext", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "id.create", use_: "create", short: "Create local DID material for bootstrap or migration", long: "", aliases: &[], phase: "phase2", hidden: true, implemented: true, handler: "id.create", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("name", "string", "Identity display name", required), flag!("identity", "string", "Identity alias override")] },
        CommandSpec { name: "id.register", use_: "register", short: "Register a handle-backed user identity", long: "", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.register", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("handle", "string", "Handle local part", required), flag!("phone", "string", "Phone number for registration"), flag!("email", "string", "Email address for registration"), flag!("otp", "string", "Verification code"), flag!("invite-code", "string", "Invite code if required"), flag!("wait", "bool", "Wait for email verification before completing registration")] },
        CommandSpec { name: "id.bind", use_: "bind", short: "Bind phone or email to the current identity", long: "", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.bind", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("phone", "string", "Phone number to bind"), flag!("email", "string", "Email address to bind"), flag!("otp", "string", "Verification code"), flag!("wait", "bool", "Wait for email verification before completing the bind")] },
        CommandSpec { name: "id.refresh-token", use_: "refresh-token", short: "Refresh the stored JWT for an identity using DID auth", long: "Refresh the selected identity's stored JWT by calling did-auth.get_me with DID credentials and persisting the newly returned bearer token. This command intentionally bypasses the previously stored bearer token instead of deleting local auth state first.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.refresh-token", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "id.resolve", use_: "resolve", short: "Resolve a DID or handle", long: "", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.resolve", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("handle", "string", "Handle to resolve"), flag!("did", "string", "DID to resolve")] },
        CommandSpec { name: "id.recover", use_: "recover", short: "Recover a handle with phone verification", long: "", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.recover", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("handle", "string", "Handle local part", required), flag!("phone", "string", "Recovery phone number", required), flag!("otp", "string", "Verification code")] },
        CommandSpec { name: "id.replace-did", use_: "replace-did", short: "Dangerously replace a handle DID with a new e1 DID", long: "Dangerous command: generates a new e1 DID and key material, replaces the selected handle identity's current DID through did-auth.replace_did, and rebinds local SQLite owner state. Select the target with the global --identity flag and run with --dry-run before executing.", aliases: &[], phase: "phase3", hidden: true, implemented: true, handler: "id.replace-did", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("is-public", "bool", "Override the public visibility flag"), flag!("is-agent", "bool", "Override the agent flag"), flag!("role", "string", "Override the role value; pass an empty string to clear it"), flag!("endpoint-url", "string", "Override the endpoint URL; pass an empty string to clear it")] },
        CommandSpec { name: "id.list", use_: "list", short: "List local identities", long: "", aliases: &[], phase: "phase2", hidden: false, implemented: true, handler: "id.list", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[] },
        CommandSpec { name: "id.current", use_: "current", short: "Show the default identity", long: "", aliases: &[], phase: "phase2", hidden: false, implemented: true, handler: "id.current", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[] },
        CommandSpec { name: "id.use", use_: "use <identity>", short: "Switch the default identity", long: "", aliases: &[], phase: "phase2", hidden: false, implemented: true, handler: "id.use", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "id.device", use_: "device", short: "Inspect and authorize devices for one DID", long: "AWiki-local device management. These commands are rollout-gated and do not add AWiki-internal checkpoints or secrets to cross-domain ANP payloads.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "id.device.list", use_: "list", short: "List authorized devices and pending Join requests", long: "Returns a safe Device Registry projection without tokens, key material, internal checkpoints, or document hashes.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.device.list", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[] },
        CommandSpec { name: "id.device.join", use_: "join", short: "Run the gated new-device Join flow", long: "Join state is restart-safe. Account verification grants are read only from AWIKI_ACCOUNT_VERIFICATION_TOKEN and are never accepted as command arguments or returned in output.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "id.device.join.sessions", use_: "sessions", short: "List restart-safe local Join sessions", long: "Lists only the safe host projection; transcript hashes, challenges, tokens, and private material stay internal.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.device.join.sessions", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[] },
        CommandSpec { name: "id.device.join.start", use_: "start", short: "Create a pending Join request as a new device", long: "Requires AWIKI_MULTI_DEVICE_JOIN_ENABLED=1 and a short-lived AWIKI_ACCOUNT_VERIFICATION_TOKEN environment value. The verification grant is never accepted in argv or emitted.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.device.join.start", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("did", "string", "Existing account DID", required), flag!("operation-id", "string", "Caller idempotency operation id", required), flag!("ttl-seconds", "int", "Join session lifetime in seconds", default = "600")] },
        CommandSpec { name: "id.device.join.poll", use_: "poll", short: "Advance and inspect a local Join session", long: "By default advances the new-device side. Pass --admin for the selected management identity; a locally derived short-lived SAS is shown only when available.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.device.join.poll", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("session", "string", "Join session id", required), flag!("admin", "bool", "Poll the management-device side")] },
        CommandSpec { name: "id.device.join.claim", use_: "claim", short: "Claim a pending Join and prepare its challenge", long: "Uses the selected active management identity. Pairing secrets and challenge plaintext never cross the CLI boundary.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.device.join.claim", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("session", "string", "Join session id", required), flag!("operation-id", "string", "Caller idempotency operation id", required), flag!("challenge-ttl-seconds", "int", "Challenge lifetime in seconds", default = "300")] },
        CommandSpec { name: "id.device.join.approve", use_: "approve", short: "Interactively approve a verified device Join", long: "Requires a foreground TTY. The user must type the locally derived SAS and APPROVE. The one-time approval handle is created and consumed in-process and never printed.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.device.join.approve", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("session", "string", "Join session id", required), flag!("role", "string", "Authorize the new device as member or admin", required, choices = ["member", "admin"])] },
        CommandSpec { name: "id.device.join.cancel", use_: "cancel", short: "Cancel one local Join side", long: "Cancels the new-device side by default; pass --admin for the selected management identity.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.device.join.cancel", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("session", "string", "Join session id", required), flag!("admin", "bool", "Cancel the management-device side")] },
        CommandSpec { name: "id.device.revoke", use_: "revoke", short: "Permanently revoke one other authorized device", long: "Requires AWIKI_MULTI_DEVICE_DEVICE_REVOKE_ENABLED=1 and a foreground TTY. Only a ready management device may revoke; self-revocation and revoking the last ready admin fail closed in Core. Output contains only DID, target device ID, and revoked status.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.device.revoke", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("device", "string", "Authorized target protocol device ID", required)] },
        CommandSpec { name: "id.device.root-key", use_: "root-key", short: "Transfer DID root control to an authorized management device", long: "AWiki-local control plane. Root material and encrypted-inner JSON never enter argv or command output.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "id.device.root-key.send", use_: "send", short: "Interactively send root control to one management device", long: "Requires AWIKI_MULTI_DEVICE_ROOT_TRANSFER_ENABLED=1 and a foreground TTY. Uses an existing exact-device P5 v2 session. --message-id is the sole idempotency key; there is no transfer_id.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.device.root-key.send", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("device", "string", "Authorized recipient protocol device ID", required), flag!("message-id", "string", "Direct message ID and idempotency key", required)] },
        CommandSpec { name: "id.device.root-key.list", use_: "list", short: "List local root-control delivery and import status", long: "Reads restart-safe Core status only. It never opens or prints root material, encrypted control payloads, private sidecars, or internal checkpoints.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.device.root-key.list", side_effect: false, outputs: &["json", "pretty"], flags: &[flag!("include-completed", "bool", "Include completed root-control operations")] },
        CommandSpec { name: "id.device.root-key.retry", use_: "retry", short: "Interactively retry one persisted root-control operation", long: "Requires a foreground TTY and reuses the exact persisted ciphertext selected only by --message-id. Recipient, secret, sidecar, and inner JSON overrides are forbidden.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.device.root-key.retry", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("message-id", "string", "Existing Direct message ID to retry", required)] },
        CommandSpec { name: "id.recovery", use_: "recovery", short: "Recover an AWiki Handle by creating a new DID", long: "AWiki-local begin/status/cancel/finalize lifecycle. It never restores the old root key or copies Direct/MLS state. Verification grants are accepted only through process environment variables.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "id.recovery.sessions", use_: "sessions", short: "List restart-safe local Handle Recovery sessions", long: "Returns only secret-free local progress. Tokens, proofs, generated documents, private keys and internal checkpoints are never rendered.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.recovery.sessions", side_effect: false, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "id.recovery.begin", use_: "begin", short: "Begin cooling-period recovery for one Handle", long: "Requires AWIKI_HANDLE_RECOVERY_BEGIN_VERIFICATION_TOKEN in the process environment. OTP success only creates a Recovery Session; it does not change the Handle or activate a device.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.recovery.begin", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("handle", "string", "Existing AWiki Handle to recover", required)] },
        CommandSpec { name: "id.recovery.status", use_: "status", short: "Refresh one Handle Recovery session", long: "Reads the authoritative Recovery phase without revealing the session token or control-plane checkpoints.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.recovery.status", side_effect: false, outputs: &["json", "pretty"], flags: &[flag!("session", "string", "Recovery Session id", required)] },
        CommandSpec { name: "id.recovery.cancel", use_: "cancel", short: "Cancel a pending Recovery from the selected old admin", long: "Requires a foreground TTY and current ready-admin authority. Local dismissal is not cancellation; this command submits the signed server-authoritative cancel.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.recovery.cancel", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("session", "string", "Recovery Session id", required)] },
        CommandSpec { name: "id.recovery.finalize", use_: "finalize", short: "Create and activate the replacement DID after cooling", long: "Requires AWIKI_HANDLE_RECOVERY_FINALIZE_VERIFICATION_TOKEN and a foreground TTY. Core creates new root/device keys locally; the old root, Ratchet and MLS state are never recovered or copied.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.recovery.finalize", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("session", "string", "Recovery Session id", required)] },
        CommandSpec { name: "id.recovery.activate", use_: "activate", short: "Resume local activation and clear its pending marker", long: "Idempotently persists the already-created replacement identity, then clears the restart marker only after local activation succeeds.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.recovery.activate", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("session", "string", "Recovery Session id", required)] },
        CommandSpec { name: "id.profile", use_: "profile", short: "Read or update DID profile data", long: "", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "id.profile.get", use_: "get", short: "Get DID profile data", long: "", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.profile.get", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("self", "bool", "Read the active identity profile"), flag!("handle", "string", "Read a profile by handle"), flag!("did", "string", "Read a profile by DID")] },
        CommandSpec { name: "id.profile.set", use_: "set", short: "Update DID profile data", long: "", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.profile.set", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("display-name", "string", "Profile display name"), flag!("bio", "string", "Profile bio"), flag!("tags", "string", "Comma-separated tags"), flag!("markdown", "string", "Inline markdown body"), flag!("markdown-file", "string", "Markdown file path"), flag!("avatar-uri", "string", "Profile avatar URI"), flag!("avatar-url", "string", "Compatibility alias for --avatar-uri", deprecated)] },
        CommandSpec { name: "id.import-v1", use_: "import-v1", short: "Import credentials from the v1 awiki-agent-id-message layout", long: "", aliases: &[], phase: "phase2", hidden: false, implemented: true, handler: "id.import-v1", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("name", "string", "Import one legacy identity by name"), flag!("all", "bool", "Import all detected legacy identities")] },
        CommandSpec { name: "msg", use_: "msg", short: "Messaging commands", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "msg.send", use_: "send", short: "Send a direct or group message", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "msg.send", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("to", "string", "Direct message target"), flag!("group", "string", "Group target"), flag!("text", "string", "Inline message text or attachment caption"), flag!("text-file", "string", "Message body or attachment caption file path"), flag!("payload", "string", "Inline JSON object message payload"), flag!("payload-file", "string", "JSON object message payload file path"), flag!("file", "string", "Attachment file path"), flag!("mime-type", "string", "Attachment MIME type override"), flag!("type", "string", "Message type", default = "text"), flag!("secure", "string", "Secure mode", default = "off", choices = ["off", "required"]), flag!("client-message-id", "string", "Client message id for idempotent sends"), flag!("idempotency-key", "string", "Delivery idempotency key") ] },
        CommandSpec { name: "msg.attachment", use_: "attachment", short: "Attachment commands", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "msg.attachment.download", use_: "download", short: "Download one attachment from a direct or group message", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "msg.attachment.download", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("with", "string", "Direct peer DID or handle"), flag!("group", "string", "Group DID"), flag!("message-id", "string", "Visible message id or raw message_id", required), flag!("attachment-id", "string", "Attachment id when the message contains multiple attachments"), flag!("output", "string", "Output file path", required)] },
        CommandSpec { name: "msg.inbox", use_: "inbox", short: "Read inbox messages", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "msg.inbox", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("scope", "string", "Message scope", default = "all", choices = ["all", "direct", "group"]), flag!("with", "string", "Direct peer filter"), flag!("group", "string", "Group filter"), flag!("unread", "bool", "Only unread messages"), flag!("limit", "int", "Maximum number of results", default = "20"), flag!("mark-read", "bool", "Mark returned messages as read")] },
        CommandSpec { name: "msg.history", use_: "history", short: "Read message history", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "msg.history", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("with", "string", "Direct peer DID or handle"), flag!("group", "string", "Group DID"), flag!("limit", "int", "Maximum number of rows", default = "50"), flag!("cursor", "string", "Pagination cursor")] },
        CommandSpec { name: "msg.mark-read", use_: "mark-read [MESSAGE_ID...]", short: "Mark messages as read", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "msg.mark-read", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "mail", use_: "mail", short: "Mail commands", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "mail.inbox", use_: "inbox", short: "List mail inbox messages", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "mail.inbox", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("folder", "string", "Mailbox folder", default = "inbox"), flag!("unread", "bool", "Only unread messages"), flag!("limit", "int", "Maximum number of results", default = "20"), flag!("offset", "int", "Pagination offset", default = "0")] },
        CommandSpec { name: "mail.notify", use_: "notify", short: "List recent mail notification messages", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "mail.notify", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("limit", "int", "Maximum number of notifications", default = "20")] },
        CommandSpec { name: "mail.read", use_: "read", short: "Read one mail message", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "mail.read", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("id", "string", "Message id", required)] },
        CommandSpec { name: "mail.mark-read", use_: "mark-read [MESSAGE_ID...]", short: "Mark mail messages as read", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "mail.mark-read", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "mail.account", use_: "account", short: "Show mailbox account info", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "mail.account", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[] },
        CommandSpec { name: "mail.send", use_: "send", short: "Send a mail message", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "mail.send", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("to", "string", "Recipient addresses (comma-separated)", required), flag!("cc", "string", "CC addresses (comma-separated)"), flag!("subject", "string", "Mail subject", required), flag!("body", "string", "Plain text body", required), flag!("html", "string", "HTML body")] },
        CommandSpec { name: "mail.attachment", use_: "attachment", short: "Mail attachment commands", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "mail.attachment.download", use_: "download", short: "Download a mail attachment", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "mail.attachment.download", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("message-id", "string", "Message id", required), flag!("attachment-index", "int", "Attachment index (0-based)", default = "0"), flag!("output", "string", "Output file path")] },
        CommandSpec { name: "msg.secure", use_: "secure", short: "Secure direct messaging commands", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "msg.secure.status", use_: "status", short: "Inspect secure messaging status", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "msg.secure.status", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("with", "string", "Target peer DID or handle")] },
        CommandSpec { name: "msg.secure.init", use_: "init", short: "Initialize a secure session", long: "", aliases: &[], phase: "phase5", hidden: true, implemented: true, handler: "msg.secure.init", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("with", "string", "Target peer DID or handle", required)] },
        CommandSpec { name: "msg.secure.repair", use_: "repair", short: "Repair a secure session", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "msg.secure.repair", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("with", "string", "Target peer DID or handle", required)] },
        CommandSpec { name: "msg.secure.failed", use_: "failed", short: "List failed secure outbox items", long: "", aliases: &[], phase: "phase5", hidden: true, implemented: true, handler: "msg.secure.failed", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[] },
        CommandSpec { name: "msg.secure.retry", use_: "retry <OUTBOX_ID>", short: "Retry one failed secure outbox item", long: "", aliases: &[], phase: "phase5", hidden: true, implemented: true, handler: "msg.secure.retry", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "msg.secure.drop", use_: "drop <OUTBOX_ID>", short: "Drop one failed secure outbox item", long: "", aliases: &[], phase: "phase5", hidden: true, implemented: true, handler: "msg.secure.drop", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "group", use_: "group", short: "Group lifecycle commands", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "group.create", use_: "create", short: "Create a new group", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "group.create", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("name", "string", "Group display name", required), flag!("description", "string", "Group description"), flag!("avatar-uri", "string", "Group avatar URI"), flag!("discoverability", "string", "Discoverability mode", default = "private"), flag!("admission-mode", "string", "Admission mode", default = "open-join"), flag!("secure", "string", "Group security requirement", default = "off", choices = ["off", "required"]), flag!("message-security-profile", "string", "Message security profile", default = "transport-protected", choices = ["transport-protected", "group-e2ee"], deprecated), flag!("e2ee", "bool", "Alias for --secure required", deprecated), flag!("slug", "string", "Group slug"), flag!("goal", "string", "Group goal"), flag!("rules", "string", "Group rules"), flag!("message-prompt", "string", "Default group prompt"), flag!("doc-url", "string", "Group document URL"), flag!("attachments-allowed", "bool", "Allow attachments"), flag!("max-members", "string", "Maximum group members"), flag!("member-max-messages", "int", "Per-member message limit"), flag!("member-max-total-chars", "int", "Per-member total char limit")] },
        CommandSpec { name: "group.get", use_: "get", short: "Show group details", long: "", aliases: &["show"], phase: "phase5", hidden: false, implemented: true, handler: "group.get", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("group", "string", "Group DID", required)] },
        CommandSpec { name: "group.join", use_: "join", short: "Join an open group", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "group.join", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("group", "string", "Group DID", required), flag!("reason", "string", "Join reason")] },
        CommandSpec { name: "group.add", use_: "add", short: "Add a member to a group", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "group.add", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("group", "string", "Group DID", required), flag!("member", "string", "Member DID or handle", required), flag!("role", "string", "Member role", default = "member"), flag!("secure", "string", "Group security requirement", default = "off", choices = ["off", "required"]), flag!("e2ee", "bool", "Alias for --secure required", deprecated)] },
        CommandSpec { name: "group.remove", use_: "remove", short: "Remove a member from a group", long: "", aliases: &["kick"], phase: "phase5", hidden: false, implemented: true, handler: "group.remove", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("group", "string", "Group DID", required), flag!("member", "string", "Member DID or handle", required), flag!("reason", "string", "Removal reason"), flag!("secure", "string", "Group security requirement", default = "off", choices = ["off", "required"]), flag!("e2ee", "bool", "Alias for --secure required", deprecated)] },
        CommandSpec { name: "group.leave", use_: "leave", short: "Leave a group", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "group.leave", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("group", "string", "Group DID", required), flag!("reason", "string", "Leave reason"), flag!("secure", "string", "Group security requirement", default = "off", choices = ["off", "required"]), flag!("e2ee", "bool", "Alias for --secure required", deprecated)] },
        CommandSpec { name: "group.update", use_: "update", short: "Update group profile or policy", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "group.update", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("group", "string", "Group DID", required), flag!("name", "string", "New group display name"), flag!("description", "string", "New group description"), flag!("avatar-uri", "string", "New group avatar URI"), flag!("discoverability", "string", "Discoverability mode"), flag!("admission-mode", "string", "Admission mode"), flag!("slug", "string", "New group slug"), flag!("goal", "string", "New group goal"), flag!("rules", "string", "New group rules"), flag!("message-prompt", "string", "New group prompt"), flag!("doc-url", "string", "New group document URL"), flag!("attachments-allowed", "bool", "Allow attachments"), flag!("max-members", "string", "Maximum group members"), flag!("member-max-messages", "int", "Per-member message limit"), flag!("member-max-total-chars", "int", "Per-member total char limit")] },
        CommandSpec { name: "group.list", use_: "list", short: "List groups joined by the active identity", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "group.list", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("limit", "int", "Maximum number of rows", default = "50")] },
        CommandSpec { name: "group.members", use_: "members", short: "List active group members", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "group.members", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("group", "string", "Group DID", required), flag!("limit", "int", "Maximum number of rows", default = "100")] },
        CommandSpec { name: "group.messages", use_: "messages", short: "List group messages", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "group.messages", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("group", "string", "Group DID", required), flag!("limit", "int", "Maximum number of rows", default = "50"), flag!("cursor", "string", "Pagination cursor")] },
        CommandSpec { name: "group.secure", use_: "secure", short: "Group secure messaging commands", long: "", aliases: &[], phase: "phase6", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "group.secure.status", use_: "status", short: "Inspect group secure status", long: "", aliases: &[], phase: "phase6", hidden: false, implemented: true, handler: "group.secure.status", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("group", "string", "Group DID", required)] },
        CommandSpec { name: "group.secure.repair", use_: "repair", short: "Repair group secure state", long: "", aliases: &[], phase: "phase6", hidden: false, implemented: true, handler: "group.secure.repair", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("group", "string", "Group DID", required), flag!("explain", "bool", "Unsupported diagnostics detail mode", deprecated)] },
        CommandSpec { name: "group.secure.diagnostics", use_: "diagnostics", short: "Unsupported group secure diagnostics", long: "", aliases: &[], phase: "phase6", hidden: true, implemented: true, handler: "group.secure.diagnostics", side_effect: false, outputs: &["json", "pretty"], flags: &[flag!("group", "string", "Group DID", required)] },
        CommandSpec { name: "group.e2ee", use_: "e2ee", short: "Deprecated group E2EE aliases and internal tools", long: "", aliases: &[], phase: "phase6", hidden: true, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "group.e2ee.status", use_: "status", short: "Deprecated alias for group secure status", long: "", aliases: &[], phase: "phase6", hidden: true, implemented: true, handler: "group.e2ee.status", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("group", "string", "Group DID", required)] },
        CommandSpec { name: "group.e2ee.publish-key-package", use_: "publish-key-package", short: "Plan a hidden/test-only group E2EE KeyPackage publish", long: "", aliases: &[], phase: "phase6", hidden: true, implemented: true, handler: "group.e2ee.publish-key-package", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("device", "string", "Local MLS device id", default = "default"), flag!("purpose", "string", "KeyPackage purpose: normal, recovery, or update", default = "normal", choices = ["normal", "recovery", "update"]), flag!("recovery", "bool", "Compatibility alias for --purpose recovery"), flag!("group", "string", "Target group DID for recovery/update KeyPackages"), flag!("contract-test", "bool", "Use non-cryptographic contract-test artifacts")] },
        CommandSpec { name: "group.e2ee.pending", use_: "pending", short: "Pull pending group E2EE P6 notices", long: "", aliases: &[], phase: "phase6", hidden: true, implemented: true, handler: "group.e2ee.pending", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("group", "string", "Optional group DID filter")] },
        CommandSpec { name: "group.e2ee.repair", use_: "repair", short: "Deprecated alias for group secure repair", long: "", aliases: &[], phase: "phase6", hidden: true, implemented: true, handler: "group.e2ee.repair", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("group", "string", "Group DID", required)] },
        CommandSpec { name: "group.e2ee.update-key", use_: "update-key", short: "Rotate an active member group E2EE key using a purpose=update KeyPackage", long: "", aliases: &[], phase: "phase6", hidden: true, implemented: true, handler: "group.e2ee.update-key", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("group", "string", "Group DID", required), flag!("member", "string", "Active member DID or handle to update", required), flag!("device", "string", "Target MLS device id", default = "default")] },
        CommandSpec { name: "group.e2ee.rejoin", use_: "rejoin", short: "Re-add a removed/left member through group add --e2ee with a fresh normal KeyPackage", long: "", aliases: &[], phase: "phase6", hidden: true, implemented: true, handler: "group.e2ee.rejoin", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("group", "string", "Group DID", required), flag!("member", "string", "Removed/left member DID or handle to rejoin", required), flag!("role", "string", "Member role", default = "member")] },
        CommandSpec { name: "group.e2ee.recover-member", use_: "recover-member", short: "Recover an active same-device group E2EE member; not for removed/left rejoin", long: "", aliases: &[], phase: "phase6", hidden: true, implemented: true, handler: "group.e2ee.recover-member", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("group", "string", "Group DID", required), flag!("member", "string", "Active member DID or handle to recover", required), flag!("device", "string", "Target MLS device id", default = "default")] },
        CommandSpec { name: "group.e2ee.process-leave-request", use_: "process-leave-request", short: "Process a pending group E2EE leave request", long: "", aliases: &[], phase: "phase6", hidden: true, implemented: true, handler: "group.e2ee.process-leave-request", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("group", "string", "Group DID", required), flag!("member", "string", "Leaving member DID or handle", required), flag!("leave-request-id", "string", "Leave request id to consume"), flag!("reason", "string", "Owner/admin processing reason")] },
        CommandSpec { name: "group.code", use_: "code", short: "Inspect or manage group join codes", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: false, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "group.code.get", use_: "get", short: "Show group join code status", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: false, handler: "stub", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("group", "string", "Group DID", required)] },
        CommandSpec { name: "group.code.refresh", use_: "refresh", short: "Rotate the current group join code", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: false, handler: "stub", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("group", "string", "Group DID", required)] },
        CommandSpec { name: "group.code.enable", use_: "enable", short: "Enable or disable group join codes", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: false, handler: "stub", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("group", "string", "Group DID", required), flag!("enabled", "bool", "Whether join codes are enabled", required)] },
        CommandSpec { name: "runtime", use_: "runtime", short: "Runtime mode, listener, and heartbeat commands", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        cmd!("runtime.status", "status", "Show runtime status", "phase7", "runtime.status"),
        CommandSpec { name: "runtime.apply", use_: "apply", short: "Apply the configured runtime state", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.apply", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "runtime.setup", use_: "setup", short: "Run runtime bootstrap and migration checks", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.setup", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("mode", "string", "Runtime mode", choices = ["http", "websocket"])] },
        CommandSpec { name: "runtime.mode", use_: "mode", short: "Inspect or update runtime mode", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: false, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "runtime.mode.get", use_: "get", short: "Get the current runtime mode", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.mode.get", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[] },
        CommandSpec { name: "runtime.mode.set", use_: "set <MODE>", short: "Set the runtime mode", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.mode.set", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "runtime.listener", use_: "listener", short: "Manage the realtime listener service", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: false, handler: "", side_effect: false, outputs: &[], flags: &[] },
        cmd!("runtime.listener.status", "status", "Show listener status", "phase7", "runtime.listener.status"),
        CommandSpec { name: "runtime.listener.install", use_: "install", short: "Install the listener service", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.listener.install", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "runtime.listener.start", use_: "start", short: "Start the listener service", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.listener.start", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "runtime.listener.stop", use_: "stop", short: "Stop the listener service", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.listener.stop", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "runtime.listener.restart", use_: "restart", short: "Restart the listener service", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.listener.restart", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "runtime.listener.uninstall", use_: "uninstall", short: "Uninstall the listener service", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.listener.uninstall", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "runtime.listener.run", use_: "run", short: "Run the listener supervisor in the foreground", long: "", aliases: &[], phase: "phase7", hidden: true, implemented: true, handler: "runtime.listener.run", side_effect: true, outputs: &[], flags: &[] },
        CommandSpec { name: "runtime.listener.service-run", use_: "service-run", short: "Run the listener supervisor under the service manager", long: "", aliases: &[], phase: "phase7", hidden: true, implemented: true, handler: "runtime.listener.service-run", side_effect: true, outputs: &[], flags: &[] },
        CommandSpec { name: "runtime.listener.config", use_: "config", short: "Inspect or update listener configuration", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: false, handler: "", side_effect: false, outputs: &[], flags: &[] },
        cmd!("runtime.listener.config.show", "show", "Show listener configuration", "phase7", "runtime.listener.config.show"),
        CommandSpec { name: "runtime.listener.config.set", use_: "set", short: "Update listener configuration", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.listener.config.set", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("enabled", "bool", "Enable or disable listener management"), flag!("auto-install", "bool", "Automatically install the listener service"), flag!("auto-start", "bool", "Automatically start the listener service")] },
        CommandSpec { name: "runtime.listener.enable", use_: "enable", short: "Enable the listener and apply runtime state", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.listener.enable", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "runtime.listener.disable", use_: "disable", short: "Disable the listener and apply runtime state", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.listener.disable", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "runtime.host-notify", use_: "host-notify", short: "Inspect or update host notification settings", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: false, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "runtime.host-notify.config", use_: "config", short: "Inspect or update host notification configuration", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: false, handler: "", side_effect: false, outputs: &[], flags: &[] },
        cmd!("runtime.host-notify.config.show", "show", "Show host notification configuration", "phase7", "runtime.host-notify.config.show"),
        CommandSpec { name: "runtime.host-notify.config.set", use_: "set", short: "Update host notification configuration", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.host-notify.config.set", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("sink", "string", "Host notification sink", choices = ["noop", "log", "file", "openclaw", "hermes"])] },
        CommandSpec { name: "runtime.host-notify.enable", use_: "enable", short: "Enable host notifications", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.host-notify.enable", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "runtime.host-notify.disable", use_: "disable", short: "Disable host notifications", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.host-notify.disable", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "runtime.host-notify.openclaw", use_: "openclaw", short: "Manage OpenClaw host notification settings", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: false, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "runtime.host-notify.openclaw.set", use_: "set", short: "Update OpenClaw host notification settings", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.host-notify.openclaw.set", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("hook-url", "string", "OpenClaw hook URL")] },
        CommandSpec { name: "runtime.host-notify.openclaw.set-token", use_: "set-token", short: "Store the OpenClaw hook token in config", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.host-notify.openclaw.set-token", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("value", "string", "OpenClaw hook token", required)] },
        CommandSpec { name: "runtime.host-notify.openclaw.clear-token", use_: "clear-token", short: "Clear the stored OpenClaw hook token", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.host-notify.openclaw.clear-token", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "runtime.host-notify.openclaw.route", use_: "route", short: "Manage OpenClaw notification routes", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: false, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "runtime.host-notify.openclaw.route.add", use_: "add", short: "Add one OpenClaw notification route", long: "Add one OpenClaw notification route using either --channel/--to or --session-key. When a new route is added, awiki-cli also sends one confirmation message to that route.", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.host-notify.openclaw.route.add", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("channel", "string", "OpenClaw delivery channel, for example feishu"), flag!("to", "string", "OpenClaw delivery target"), flag!("session-key", "string", "OpenClaw session key to parse into channel/to")] },
        CommandSpec { name: "runtime.host-notify.openclaw.route.list", use_: "list", short: "List configured OpenClaw notification routes", long: "List the configured OpenClaw notification routes. Each route is identified by channel and to.", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.host-notify.openclaw.route.list", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[] },
        CommandSpec { name: "runtime.host-notify.openclaw.route.remove", use_: "remove", short: "Remove one OpenClaw notification route", long: "Remove one OpenClaw notification route using either --channel/--to or --session-key.", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.host-notify.openclaw.route.remove", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("channel", "string", "OpenClaw delivery channel, for example feishu"), flag!("to", "string", "OpenClaw delivery target"), flag!("session-key", "string", "OpenClaw session key to parse into channel/to")] },
        CommandSpec { name: "runtime.host-notify.hermes", use_: "hermes", short: "Manage Hermes host notification settings", long: "", aliases: &["webhook"], phase: "phase7", hidden: false, implemented: false, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "runtime.host-notify.hermes.guide", use_: "guide", short: "Show a ready-to-use Hermes host notification guide", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.host-notify.hermes.guide", side_effect: false, outputs: &["json", "pretty"], flags: &[flag!("deliver", "string", "Recommended Hermes route deliver target", default = "feishu")] },
        CommandSpec { name: "runtime.host-notify.hermes.status", use_: "status", short: "Show end-to-end Hermes host notification readiness", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.host-notify.hermes.status", side_effect: false, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "runtime.host-notify.hermes.setup", use_: "setup", short: "Configure awiki-cli and local Hermes for host notifications", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.host-notify.hermes.setup", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("notify-url", "string", "Hermes notify endpoint URL"), flag!("deliver", "string", "Hermes route deliver target to persist in awiki-cli config"), flag!("secret", "string", "Hermes signing secret to store in awiki-cli config")] },
        CommandSpec { name: "runtime.host-notify.hermes.bridge", use_: "bridge", short: "Manage the local Hermes host notification bridge", long: "", aliases: &[], phase: "phase7", hidden: true, implemented: false, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "runtime.host-notify.hermes.bridge.service-run", use_: "service-run", short: "Run the Hermes host notification bridge under the service manager", long: "", aliases: &[], phase: "phase7", hidden: true, implemented: true, handler: "runtime.host-notify.hermes.bridge.service-run", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "runtime.host-notify.hermes.set", use_: "set", short: "Update Hermes host notification settings", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.host-notify.hermes.set", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("notify-url", "string", "Hermes notify endpoint URL"), flag!("deliver", "string", "Hermes route deliver target")] },
        CommandSpec { name: "runtime.host-notify.hermes.set-secret", use_: "set-secret", short: "Store the Hermes signing secret in config", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.host-notify.hermes.set-secret", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("value", "string", "Hermes signing secret", required)] },
        CommandSpec { name: "runtime.host-notify.hermes.clear-secret", use_: "clear-secret", short: "Clear the stored Hermes signing secret", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.host-notify.hermes.clear-secret", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "runtime.heartbeat", use_: "heartbeat", short: "Manage heartbeat tasks", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: false, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "runtime.heartbeat.status", use_: "status", short: "Show heartbeat status", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: false, handler: "stub", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[] },
        CommandSpec { name: "runtime.heartbeat.install", use_: "install", short: "Install heartbeat automation", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: false, handler: "stub", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("every", "string", "Heartbeat schedule", default = "15m")] },
        CommandSpec { name: "runtime.heartbeat.run-once", use_: "run-once", short: "Run heartbeat once", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: false, handler: "stub", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "people", use_: "people", short: "People, relationships, and contacts commands", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "people.search", use_: "search <QUERY>", short: "Search users", long: "", aliases: &[], phase: "phase8", hidden: false, implemented: false, handler: "stub", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[] },
        CommandSpec { name: "people.follow", use_: "follow <TARGET>", short: "Follow a user", long: "", aliases: &[], phase: "phase8", hidden: false, implemented: true, handler: "people.follow", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "people.unfollow", use_: "unfollow <TARGET>", short: "Unfollow a user", long: "", aliases: &[], phase: "phase8", hidden: false, implemented: true, handler: "people.unfollow", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "people.status", use_: "status <TARGET>", short: "Show relationship status", long: "", aliases: &[], phase: "phase8", hidden: false, implemented: true, handler: "people.status", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[] },
        CommandSpec { name: "people.followers", use_: "followers", short: "List followers", long: "", aliases: &[], phase: "phase8", hidden: false, implemented: true, handler: "people.followers", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("limit", "int", "Maximum number of rows", default = "50"), flag!("offset", "int", "Pagination offset", default = "0"), flag!("profile", "bool", "Hydrate public profiles")] },
        CommandSpec { name: "people.following", use_: "following", short: "List following", long: "", aliases: &[], phase: "phase8", hidden: false, implemented: true, handler: "people.following", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("limit", "int", "Maximum number of rows", default = "50"), flag!("offset", "int", "Pagination offset", default = "0"), flag!("profile", "bool", "Hydrate public profiles")] },
        CommandSpec { name: "people.contacts", use_: "contacts", short: "Manage local contacts", long: "", aliases: &[], phase: "phase8", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "people.contacts.list", use_: "list", short: "List local contacts", long: "", aliases: &[], phase: "phase8", hidden: false, implemented: true, handler: "people.contacts.list", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("limit", "int", "Maximum number of rows", default = "100")] },
        CommandSpec { name: "people.contacts.save", use_: "save", short: "Save a local contact", long: "", aliases: &[], phase: "phase8", hidden: false, implemented: true, handler: "people.contacts.save", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("did", "string", "Contact DID", required), flag!("handle", "string", "Contact handle"), flag!("display-name", "string", "Contact display name"), flag!("name", "string", "Compatibility alias for --display-name", deprecated), flag!("relationship", "string", "Local relationship label"), flag!("reason", "string", "Why the contact was saved")] },
        CommandSpec { name: "page", use_: "page", short: "Handle-level content page commands", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "page.create", use_: "create", short: "Create a handle-level content page", long: "", aliases: &[], phase: "phase8", hidden: false, implemented: true, handler: "page.create", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("slug", "string", "Page slug"), flag!("title", "string", "Page title"), flag!("markdown", "string", "Inline markdown body"), flag!("markdown-file", "string", "Markdown file path"), flag!("visibility", "string", "Page visibility", default = "public", choices = ["public", "draft", "unlisted"])] },
        CommandSpec { name: "page.list", use_: "list", short: "List handle-level content pages", long: "", aliases: &[], phase: "phase8", hidden: false, implemented: true, handler: "page.list", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[] },
        CommandSpec { name: "page.get", use_: "get", short: "Get one handle-level content page", long: "", aliases: &[], phase: "phase8", hidden: false, implemented: true, handler: "page.get", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("slug", "string", "Page slug", required)] },
        CommandSpec { name: "page.update", use_: "update", short: "Update a handle-level content page", long: "", aliases: &[], phase: "phase8", hidden: false, implemented: true, handler: "page.update", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("slug", "string", "Page slug", required), flag!("title", "string", "Page title"), flag!("markdown", "string", "Inline markdown body"), flag!("markdown-file", "string", "Markdown file path"), flag!("visibility", "string", "Page visibility", choices = ["public", "draft", "unlisted"])] },
        CommandSpec { name: "page.rename", use_: "rename", short: "Rename a handle-level content page slug", long: "", aliases: &[], phase: "phase8", hidden: false, implemented: true, handler: "page.rename", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("slug", "string", "Current page slug", required), flag!("to", "string", "New slug", required)] },
        CommandSpec { name: "page.delete", use_: "delete", short: "Delete a handle-level content page", long: "", aliases: &[], phase: "phase8", hidden: false, implemented: true, handler: "page.delete", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("slug", "string", "Page slug", required)] },
        CommandSpec { name: "site", use_: "site", short: "Tenant bare-domain site page commands", long: "Manage public root and Markdown pages for an explicitly named remote tenant domain. The required --domain flag selects remote site content only; it never creates, reconfigures, or switches the active local CLI tenant.", aliases: &[], phase: "phase8", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "site.root", use_: "root", short: "Manage the tenant root page", long: "Read or update the public root page for the explicit --domain. Site administration uses the active identity but does not change the active local tenant.", aliases: &[], phase: "phase8", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "site.root.get", use_: "get", short: "Get the tenant root page", long: "Read the public root Markdown for the required remote --domain. The domain is not inferred from the active tenant or identity.", aliases: &[], phase: "phase8", hidden: false, implemented: true, handler: "site.root.get", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("domain", "string", "Remote tenant bare domain; does not switch active tenant", required)] },
        CommandSpec { name: "site.root.set", use_: "set", short: "Update the tenant root page", long: "Update the public root Markdown for the required remote --domain using exactly one of --markdown or --markdown-file. This does not switch the active tenant.", aliases: &[], phase: "phase8", hidden: false, implemented: true, handler: "site.root.set", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("domain", "string", "Remote tenant bare domain; does not switch active tenant", required), flag!("markdown", "string", "Inline markdown body"), flag!("markdown-file", "string", "Markdown file path")] },
        CommandSpec { name: "site.page", use_: "page", short: "Manage tenant bare-domain pages", long: "Manage public Markdown pages for an explicit remote tenant domain. Every operation requires --domain and leaves the active local tenant unchanged.", aliases: &[], phase: "phase8", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "site.page.list", use_: "list", short: "List tenant site pages", long: "List public site pages for the required remote --domain without switching the active tenant.", aliases: &[], phase: "phase8", hidden: false, implemented: true, handler: "site.page.list", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("domain", "string", "Remote tenant bare domain; does not switch active tenant", required)] },
        CommandSpec { name: "site.page.get", use_: "get", short: "Get one tenant site page", long: "Read one public page by --slug from the required remote --domain without switching the active tenant.", aliases: &[], phase: "phase8", hidden: false, implemented: true, handler: "site.page.get", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("domain", "string", "Remote tenant bare domain; does not switch active tenant", required), flag!("slug", "string", "Page slug", required)] },
        CommandSpec { name: "site.page.create", use_: "create", short: "Create a tenant site page", long: "Create one public page under the required remote --domain using exactly one Markdown source. This does not switch the active tenant.", aliases: &[], phase: "phase8", hidden: false, implemented: true, handler: "site.page.create", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("domain", "string", "Remote tenant bare domain; does not switch active tenant", required), flag!("slug", "string", "Page slug", required), flag!("markdown", "string", "Inline markdown body"), flag!("markdown-file", "string", "Markdown file path")] },
        CommandSpec { name: "site.page.update", use_: "update", short: "Update a tenant site page", long: "Replace one public page under the required remote --domain using exactly one Markdown source. This does not switch the active tenant.", aliases: &[], phase: "phase8", hidden: false, implemented: true, handler: "site.page.update", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("domain", "string", "Remote tenant bare domain; does not switch active tenant", required), flag!("slug", "string", "Page slug", required), flag!("markdown", "string", "Inline markdown body"), flag!("markdown-file", "string", "Markdown file path")] },
        CommandSpec { name: "site.page.rename", use_: "rename", short: "Rename a tenant site page slug", long: "Rename a page slug under the required remote --domain. This changes remote site content naming only and does not switch the active tenant.", aliases: &[], phase: "phase8", hidden: false, implemented: true, handler: "site.page.rename", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("domain", "string", "Remote tenant bare domain; does not switch active tenant", required), flag!("slug", "string", "Current page slug", required), flag!("to", "string", "New slug", required)] },
        CommandSpec { name: "site.page.delete", use_: "delete", short: "Delete a tenant site page", long: "Delete one public page from the required remote --domain. This does not remove or switch any local tenant data.", aliases: &[], phase: "phase8", hidden: false, implemented: true, handler: "site.page.delete", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("domain", "string", "Remote tenant bare domain; does not switch active tenant", required), flag!("slug", "string", "Page slug", required)] },
        CommandSpec { name: "debug", use_: "debug", short: "Debugging and raw inspection commands", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "debug.db", use_: "db", short: "Database inspection helpers", long: "", aliases: &[], phase: "phase4", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "debug.db.handle-history", use_: "handle-history <HANDLE>", short: "Show the local DID history recorded for one handle", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "debug.db.handle-history", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[] },
        CommandSpec { name: "debug.db.query", use_: "query <SQL>", short: "Raw SQLite query is no longer supported", long: "", aliases: &[], phase: "phase4", hidden: false, implemented: false, handler: "stub", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[] },
        CommandSpec { name: "debug.db.import-v1", use_: "import-v1", short: "Import a legacy v1 local SQLite database", long: "", aliases: &[], phase: "phase4", hidden: false, implemented: true, handler: "debug.db.import-v1", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("path", "string", "Explicit legacy database path override")] },
        CommandSpec { name: "debug.raw", use_: "raw", short: "Raw RPC helpers", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: false, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "debug.raw.rpc", use_: "rpc", short: "Call raw RPC endpoints", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: false, handler: "stub", side_effect: false, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "debug.schema-cache", use_: "schema-cache", short: "Inspect generated schema metadata", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: false, handler: "stub", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[] },
        CommandSpec { name: "debug.logs", use_: "logs", short: "Tail runtime logs", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: false, handler: "stub", side_effect: false, outputs: &["ndjson", "pretty"], flags: &[flag!("follow", "bool", "Follow log output")] },
    ]
}

impl Serialize for FlagSpec {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("name", self.name)?;
        map.serialize_entry("type", self.flag_type)?;
        map.serialize_entry("usage", self.usage)?;
        if !self.default.is_empty() {
            map.serialize_entry("default", self.default)?;
        }
        if self.required {
            map.serialize_entry("required", &self.required)?;
        }
        if !self.choices.is_empty() {
            map.serialize_entry("choices", self.choices)?;
        }
        if self.deprecated {
            map.serialize_entry("deprecated", &self.deprecated)?;
        }
        map.end()
    }
}

impl Serialize for CommandSpec {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("name", self.name)?;
        map.serialize_entry("canonical_name", self.canonical_name())?;
        map.serialize_entry("use", self.use_)?;
        map.serialize_entry("short", self.short)?;
        if !self.long.is_empty() {
            map.serialize_entry("long", self.long)?;
        }
        if !self.aliases.is_empty() {
            map.serialize_entry("aliases", self.aliases)?;
        }
        map.serialize_entry("phase", self.phase)?;
        if self.hidden {
            map.serialize_entry("hidden", &self.hidden)?;
        }
        map.serialize_entry("implemented", &self.implemented)?;
        if !self.handler.is_empty() {
            map.serialize_entry("handler", self.handler)?;
        }
        map.serialize_entry("side_effect", &self.side_effect)?;
        if !self.outputs.is_empty() {
            map.serialize_entry("outputs", self.outputs)?;
        }
        if !self.flags.is_empty() {
            map.serialize_entry("flags", self.flags)?;
        }
        map.serialize_entry("audience", &self.audience().as_str())?;
        map.serialize_entry("primary_owner", &self.primary_owner().as_str())?;
        let secondary_owners: Vec<_> = self
            .secondary_owners()
            .iter()
            .map(|owner| owner.as_str())
            .collect();
        map.serialize_entry("secondary_owners", &secondary_owners)?;
        map.serialize_entry("cli_shell_role", &self.cli_shell_role().as_str())?;
        map.serialize_entry("direct_invocation", &self.direct_invocation())?;
        map.serialize_entry(
            "cutover",
            &CommandCutoverView {
                status: self.cutover_status(),
                default_surface: self.include_in_default_surface(),
                capability: self.cutover_status().capability(),
                required_phase: self.cutover_status().required_phase(),
            },
        )?;
        map.end()
    }
}

impl Serialize for CommandSchemaSpec<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("name", self.spec.name)?;
        map.serialize_entry("canonical_name", self.spec.canonical_name())?;
        map.serialize_entry("use", self.spec.use_)?;
        map.serialize_entry("short", self.spec.short)?;
        if !self.spec.long.is_empty() {
            map.serialize_entry("long", self.spec.long)?;
        }
        if !self.spec.aliases.is_empty() {
            map.serialize_entry("aliases", self.spec.aliases)?;
        }
        map.serialize_entry("phase", self.spec.phase)?;
        if self.spec.hidden {
            map.serialize_entry("hidden", &self.spec.hidden)?;
        }
        map.serialize_entry("implemented", &self.spec.implemented)?;
        if !self.spec.handler.is_empty() {
            map.serialize_entry("handler", self.spec.handler)?;
        }
        map.serialize_entry("side_effect", &self.spec.side_effect)?;
        if !self.spec.outputs.is_empty() {
            map.serialize_entry("outputs", self.spec.outputs)?;
        }
        let flags: Vec<_> = self
            .spec
            .flags
            .iter()
            .filter(|flag| self.include_deprecated_flags || !flag.deprecated)
            .collect();
        if !flags.is_empty() {
            map.serialize_entry("flags", &flags)?;
        }
        map.serialize_entry("audience", &self.spec.audience().as_str())?;
        map.serialize_entry("primary_owner", &self.spec.primary_owner().as_str())?;
        let secondary_owners: Vec<_> = self
            .spec
            .secondary_owners()
            .iter()
            .map(|owner| owner.as_str())
            .collect();
        map.serialize_entry("secondary_owners", &secondary_owners)?;
        map.serialize_entry("cli_shell_role", &self.spec.cli_shell_role().as_str())?;
        map.serialize_entry("direct_invocation", &self.spec.direct_invocation())?;
        map.serialize_entry(
            "cutover",
            &CommandCutoverView {
                status: self.spec.cutover_status(),
                default_surface: self.spec.include_in_default_surface(),
                capability: self.spec.cutover_status().capability(),
                required_phase: self.spec.cutover_status().required_phase(),
            },
        )?;
        map.end()
    }
}

impl Serialize for SchemaSpecList<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::All(specs) => specs.serialize(serializer),
            Self::Default(specs) => specs.serialize(serializer),
        }
    }
}

impl Serialize for SchemaCommandSpec<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::All(spec) => spec.serialize(serializer),
            Self::Default(spec) => spec.serialize(serializer),
        }
    }
}

impl Serialize for DirectInvocationPolicy {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("policy", self.kind())?;
        match self {
            DirectInvocationPolicy::StableUnsupported { capability, phase } => {
                map.serialize_entry("capability", capability)?;
                map.serialize_entry("required_phase", phase)?;
            }
            DirectInvocationPolicy::Removed { replacement } => {
                if let Some(replacement) = replacement {
                    map.serialize_entry("replacement", replacement)?;
                } else {
                    map.serialize_entry("replacement", &Option::<&str>::None)?;
                }
            }
            DirectInvocationPolicy::DeprecatedAlias { replacement, until } => {
                map.serialize_entry("replacement", replacement)?;
                map.serialize_entry("until", until)?;
            }
            DirectInvocationPolicy::Allow
            | DirectInvocationPolicy::AllowWithWarning
            | DirectInvocationPolicy::RequireDiagnosticGate
            | DirectInvocationPolicy::RequireMigrationGate
            | DirectInvocationPolicy::RequireInternalServiceGate => {}
        }
        map.end()
    }
}

impl Serialize for CutoverStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        CommandCutoverView {
            status: *self,
            default_surface: self.include_in_default_surface(),
            capability: self.capability(),
            required_phase: self.required_phase(),
        }
        .serialize(serializer)
    }
}

impl Serialize for CommandCutoverView<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("status", self.status.kind())?;
        map.serialize_entry("default_surface", &self.default_surface)?;
        if let Some(capability) = self.capability {
            map.serialize_entry("capability", capability)?;
        }
        if let Some(phase) = self.required_phase {
            map.serialize_entry("required_phase", phase)?;
        }
        map.end()
    }
}
