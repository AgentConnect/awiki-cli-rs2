use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct FlagSpec {
    pub name: &'static str,
    #[serde(rename = "type")]
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

impl CommandSpec {
    pub fn json_use(&self) -> &'static str {
        self.use_
    }
}

pub fn specs() -> Vec<CommandSpec> {
    default_specs().to_vec()
}

pub fn lookup(raw: &str) -> Option<CommandSpec> {
    let needle = normalize_name(raw);
    default_specs()
        .iter()
        .find(|spec| normalize_name(spec.name) == needle)
        .cloned()
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

pub fn normalize_name(raw: &str) -> String {
    raw.trim()
        .strip_prefix("awiki-cli")
        .unwrap_or(raw.trim())
        .trim()
        .replace(' ', ".")
        .trim_matches('.')
        .to_ascii_lowercase()
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
        cmd!("schema", "schema [command]", "Show the static command contract", "phase1", "schema"),
        cmd!("doctor", "doctor", "Run baseline environment and storage diagnostics", "phase1", "doctor"),
        cmd!("version", "version", "Show build information", "phase1", "version"),
        CommandSpec { name: "upgrade", use_: "upgrade", short: "Upgrade awiki-cli to the latest supported release", long: "", aliases: &[], phase: "phase2", hidden: false, implemented: true, handler: "upgrade", side_effect: true, outputs: &["json", "pretty", "table"], flags: &[] },
        CommandSpec { name: "init", use_: "init", short: "Initialize the awiki-cli workspace and config.yaml", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "init", side_effect: true, outputs: &["json", "pretty", "table"], flags: &[] },
        CommandSpec { name: "completion", use_: "completion", short: "Generate shell completion scripts", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "completion.bash", use_: "bash", short: "Generate Bash completion", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "completion.bash", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "completion.zsh", use_: "zsh", short: "Generate Zsh completion", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "completion.zsh", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "completion.fish", use_: "fish", short: "Generate Fish completion", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "completion.fish", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "completion.powershell", use_: "powershell", short: "Generate PowerShell completion", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "completion.powershell", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "config", use_: "config", short: "Inspect resolved CLI configuration", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        cmd!("config.show", "show", "Show resolved configuration values", "phase1", "config.show"),
        CommandSpec { name: "config.set", use_: "set", short: "Update persistent CLI configuration", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "config.set", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("did-domain", "string", "Bare DID provider domain to persist in services.did_domain")] },
        CommandSpec { name: "id", use_: "id", short: "Identity lifecycle commands", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "id.status", use_: "status", short: "Show identity status", long: "", aliases: &[], phase: "phase2", hidden: false, implemented: true, handler: "id.status", side_effect: false, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "id.create", use_: "create", short: "Create local DID material for bootstrap or migration", long: "", aliases: &[], phase: "phase2", hidden: true, implemented: true, handler: "id.create", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("name", "string", "Identity display name", required), flag!("identity", "string", "Identity alias override")] },
        CommandSpec { name: "id.refresh-token", use_: "refresh-token", short: "Refresh the stored JWT for an identity using DID auth", long: "Refresh the selected identity's stored JWT by calling did-auth.get_me with DID credentials and persisting the newly returned bearer token. This command intentionally bypasses the previously stored bearer token instead of deleting local auth state first.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.refresh-token", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "id.replace-did", use_: "replace-did", short: "Dangerously replace a handle DID with a new e1 DID", long: "Dangerous command: generates a new e1 DID and key material, replaces the selected handle identity's current DID through did-auth.replace_did, and rebinds local SQLite owner state. Select the target with the global --identity flag and run with --dry-run before executing.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.replace-did", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("is-public", "bool", "Override the public visibility flag"), flag!("is-agent", "bool", "Override the agent flag"), flag!("role", "string", "Override the role value; pass an empty string to clear it"), flag!("endpoint-url", "string", "Override the endpoint URL; pass an empty string to clear it")] },
        CommandSpec { name: "id.list", use_: "list", short: "List local identities", long: "", aliases: &[], phase: "phase2", hidden: false, implemented: true, handler: "id.list", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[] },
        CommandSpec { name: "id.current", use_: "current", short: "Show the default identity", long: "", aliases: &[], phase: "phase2", hidden: false, implemented: true, handler: "id.current", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[] },
        CommandSpec { name: "id.use", use_: "use <identity>", short: "Switch the default identity", long: "", aliases: &[], phase: "phase2", hidden: false, implemented: true, handler: "id.use", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "id.import-v1", use_: "import-v1", short: "Import credentials from the v1 awiki-agent-id-message layout", long: "", aliases: &[], phase: "phase2", hidden: false, implemented: true, handler: "id.import-v1", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("name", "string", "Import one legacy identity by name"), flag!("all", "bool", "Import all detected legacy identities")] },
        CommandSpec { name: "msg", use_: "msg", short: "Messaging commands", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "msg.send", use_: "send", short: "Send a direct or group message", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "msg.send", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("to", "string", "Direct message target"), flag!("group", "string", "Group target"), flag!("text", "string", "Inline message text or attachment caption"), flag!("text-file", "string", "Message body or attachment caption file path"), flag!("file", "string", "Attachment file path"), flag!("mime-type", "string", "Attachment MIME type override"), flag!("type", "string", "Message type", default = "text"), flag!("secure", "string", "Secure mode", default = "off")] },
        CommandSpec { name: "mail", use_: "mail", short: "Mail commands", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "mail.inbox", use_: "inbox", short: "List mail inbox messages", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "mail.inbox", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("folder", "string", "Mailbox folder", default = "inbox"), flag!("unread", "bool", "Only unread messages"), flag!("limit", "int", "Maximum number of results", default = "20"), flag!("offset", "int", "Pagination offset", default = "0")] },
        CommandSpec { name: "mail.notify", use_: "notify", short: "List recent mail notification messages", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "mail.notify", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("limit", "int", "Maximum number of notifications", default = "20")] },
        CommandSpec { name: "mail.read", use_: "read", short: "Read one mail message", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "mail.read", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[flag!("id", "string", "Message id", required)] },
        CommandSpec { name: "mail.mark-read", use_: "mark-read [MESSAGE_ID...]", short: "Mark mail messages as read", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "mail.mark-read", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "mail.account", use_: "account", short: "Show mailbox account info", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "mail.account", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[] },
        CommandSpec { name: "mail.send", use_: "send", short: "Send a mail message", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "mail.send", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("to", "string", "Recipient addresses (comma-separated)", required), flag!("cc", "string", "CC addresses (comma-separated)"), flag!("subject", "string", "Mail subject", required), flag!("body", "string", "Plain text body", required), flag!("html", "string", "HTML body")] },
        CommandSpec { name: "mail.attachment", use_: "attachment", short: "Mail attachment commands", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "mail.attachment.download", use_: "download", short: "Download a mail attachment", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "mail.attachment.download", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("message-id", "string", "Message id", required), flag!("attachment-index", "int", "Attachment index (0-based)", default = "0"), flag!("output", "string", "Output file path")] },
        CommandSpec { name: "group", use_: "group", short: "Group lifecycle commands", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "group.create", use_: "create", short: "Create a new group", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "group.create", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("name", "string", "Group display name", required), flag!("description", "string", "Group description"), flag!("discoverability", "string", "Discoverability mode", default = "private"), flag!("admission-mode", "string", "Admission mode", default = "open-join"), flag!("message-security-profile", "string", "Message security profile", default = "transport-protected"), flag!("e2ee", "bool", "Alias for --message-security-profile group-e2ee"), flag!("slug", "string", "Group slug"), flag!("goal", "string", "Group goal"), flag!("rules", "string", "Group rules"), flag!("message-prompt", "string", "Default group prompt"), flag!("doc-url", "string", "Group document URL"), flag!("attachments-allowed", "bool", "Allow attachments"), flag!("max-members", "string", "Maximum group members"), flag!("member-max-messages", "int", "Per-member message limit"), flag!("member-max-total-chars", "int", "Per-member total char limit")] },
        CommandSpec { name: "group.update", use_: "update", short: "Update group profile or policy", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "group.update", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("group", "string", "Group DID", required), flag!("name", "string", "New group display name"), flag!("description", "string", "New group description"), flag!("discoverability", "string", "Discoverability mode"), flag!("admission-mode", "string", "Admission mode"), flag!("slug", "string", "New group slug"), flag!("goal", "string", "New group goal"), flag!("rules", "string", "New group rules"), flag!("message-prompt", "string", "New group prompt"), flag!("doc-url", "string", "New group document URL"), flag!("attachments-allowed", "bool", "Allow attachments"), flag!("max-members", "string", "Maximum group members"), flag!("member-max-messages", "int", "Per-member message limit"), flag!("member-max-total-chars", "int", "Per-member total char limit")] },
        CommandSpec { name: "group.join", use_: "join", short: "Join an open group", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "group.join", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("group", "string", "Group DID", required), flag!("reason", "string", "Join reason")] },
        CommandSpec { name: "runtime", use_: "runtime", short: "Runtime mode, listener, and heartbeat commands", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        cmd!("runtime.status", "status", "Show runtime status", "phase7", "runtime.status"),
        CommandSpec { name: "runtime.apply", use_: "apply", short: "Apply runtime policy", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.apply", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "runtime.setup", use_: "setup", short: "Run runtime bootstrap and migration checks", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.setup", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("mode", "string", "Runtime mode")] },
        CommandSpec { name: "runtime.mode", use_: "mode", short: "Get or set runtime mode", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        cmd!("runtime.mode.get", "get", "Show runtime mode", "phase7", "runtime.mode.get"),
        CommandSpec { name: "runtime.mode.set", use_: "set <http|websocket>", short: "Set runtime mode", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.mode.set", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "runtime.listener", use_: "listener", short: "Manage the realtime listener service", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        cmd!("runtime.listener.status", "status", "Show listener status", "phase7", "runtime.listener.status"),
        CommandSpec { name: "runtime.listener.install", use_: "install", short: "Install listener service", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.listener.install", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "runtime.listener.start", use_: "start", short: "Start listener service", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.listener.start", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "runtime.listener.stop", use_: "stop", short: "Stop listener service", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.listener.stop", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "runtime.listener.restart", use_: "restart", short: "Restart listener service", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.listener.restart", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "runtime.listener.uninstall", use_: "uninstall", short: "Uninstall listener service", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.listener.uninstall", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "runtime.listener.enable", use_: "enable", short: "Enable listener policy", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.listener.enable", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "runtime.listener.disable", use_: "disable", short: "Disable listener policy", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.listener.disable", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "runtime.listener.config", use_: "config", short: "Inspect or update listener configuration", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        cmd!("runtime.listener.config.show", "show", "Show listener configuration", "phase7", "runtime.listener.config.show"),
        CommandSpec { name: "runtime.listener.config.set", use_: "set", short: "Update listener configuration", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.listener.config.set", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("enabled", "bool", "Enable or disable listener management"), flag!("auto-install", "bool", "Automatically install the listener service"), flag!("auto-start", "bool", "Automatically start the listener service")] },
        CommandSpec { name: "runtime.host-notify", use_: "host-notify", short: "Configure host notifications", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "runtime.host-notify.config", use_: "config", short: "Inspect or update host notification configuration", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        cmd!("runtime.host-notify.config.show", "show", "Show host notification configuration", "phase7", "runtime.host-notify.config.show"),
        CommandSpec { name: "runtime.host-notify.config.set", use_: "set", short: "Set host notification sink", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.host-notify.config.set", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("sink", "string", "Host notification sink")] },
        CommandSpec { name: "runtime.host-notify.openclaw", use_: "openclaw", short: "Configure OpenClaw host notifications", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "runtime.host-notify.openclaw.set", use_: "set", short: "Update OpenClaw host notification settings", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.host-notify.openclaw.set", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("hook-url", "string", "OpenClaw hook URL")] },
        CommandSpec { name: "runtime.host-notify.openclaw.set-token", use_: "set-token", short: "Store OpenClaw hook token", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.host-notify.openclaw.set-token", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("value", "string", "OpenClaw hook token")] },
        CommandSpec { name: "runtime.host-notify.openclaw.clear-token", use_: "clear-token", short: "Clear OpenClaw hook token", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.host-notify.openclaw.clear-token", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "page", use_: "page", short: "Handle-level content page commands", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "page.create", use_: "create", short: "Create a handle-level content page", long: "", aliases: &[], phase: "phase8", hidden: false, implemented: true, handler: "page.create", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("slug", "string", "Page slug"), flag!("title", "string", "Page title"), flag!("markdown", "string", "Inline markdown body"), flag!("markdown-file", "string", "Markdown file path"), flag!("visibility", "string", "Page visibility", default = "public")] },
        CommandSpec { name: "debug", use_: "debug", short: "Debug local state and storage", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "debug.db", use_: "db", short: "Debug the local SQLite store", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "debug.db.query", use_: "query <sql>", short: "Run a safe single-statement SQLite query", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "debug.db.query", side_effect: false, outputs: &["json", "pretty", "table"], flags: &[] },
        CommandSpec { name: "debug.db.import-v1", use_: "import-v1", short: "Import a legacy v1 SQLite database", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "debug.db.import-v1", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("path", "string", "Legacy data root or sqlite file path")] },
    ]
}

impl Serialize for CommandSpec {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("name", self.name)?;
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
        map.end()
    }
}
