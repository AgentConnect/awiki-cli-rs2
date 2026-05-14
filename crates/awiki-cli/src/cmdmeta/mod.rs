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
        CommandSpec { name: "init", use_: "init", short: "Initialize the awiki-cli workspace and config.yaml", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "init", side_effect: true, outputs: &["json", "pretty", "table"], flags: &[] },
        CommandSpec { name: "completion", use_: "completion", short: "Generate shell completion scripts", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "completion.bash", use_: "bash", short: "Generate Bash completion", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "completion.bash", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "completion.zsh", use_: "zsh", short: "Generate Zsh completion", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "completion.zsh", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "completion.fish", use_: "fish", short: "Generate Fish completion", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "completion.fish", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "completion.powershell", use_: "powershell", short: "Generate PowerShell completion", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "completion.powershell", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "config", use_: "config", short: "Inspect resolved CLI configuration", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        cmd!("config.show", "show", "Show resolved configuration values", "phase1", "config.show"),
        CommandSpec { name: "id", use_: "id", short: "Identity lifecycle commands", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "id.create", use_: "create", short: "Create local DID material for bootstrap or migration", long: "", aliases: &[], phase: "phase2", hidden: true, implemented: true, handler: "id.create", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("name", "string", "Identity display name", required), flag!("identity", "string", "Identity alias override")] },
        CommandSpec { name: "id.refresh-token", use_: "refresh-token", short: "Refresh the stored JWT for an identity using DID auth", long: "Refresh the selected identity's stored JWT by calling did-auth.get_me with DID credentials and persisting the newly returned bearer token. This command intentionally bypasses the previously stored bearer token instead of deleting local auth state first.", aliases: &[], phase: "phase3", hidden: false, implemented: true, handler: "id.refresh-token", side_effect: true, outputs: &["json", "pretty"], flags: &[] },
        CommandSpec { name: "msg", use_: "msg", short: "Messaging commands", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "msg.send", use_: "send", short: "Send a direct or group message", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "msg.send", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("to", "string", "Direct message target"), flag!("group", "string", "Group target"), flag!("text", "string", "Inline message text or attachment caption"), flag!("text-file", "string", "Message body or attachment caption file path"), flag!("file", "string", "Attachment file path"), flag!("mime-type", "string", "Attachment MIME type override"), flag!("type", "string", "Message type", default = "text"), flag!("secure", "string", "Secure mode", default = "off")] },
        CommandSpec { name: "group", use_: "group", short: "Group lifecycle commands", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "group.join", use_: "join", short: "Join an open group", long: "", aliases: &[], phase: "phase5", hidden: false, implemented: true, handler: "group.join", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("group", "string", "Group DID", required), flag!("reason", "string", "Join reason")] },
        CommandSpec { name: "runtime", use_: "runtime", short: "Runtime mode, listener, and heartbeat commands", long: "", aliases: &[], phase: "phase1", hidden: false, implemented: true, handler: "", side_effect: false, outputs: &[], flags: &[] },
        cmd!("runtime.status", "status", "Show runtime status", "phase7", "runtime.status"),
        CommandSpec { name: "runtime.setup", use_: "setup", short: "Run runtime bootstrap and migration checks", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.setup", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("mode", "string", "Runtime mode")] },
        CommandSpec { name: "runtime.listener", use_: "listener", short: "Manage the realtime listener service", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: false, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "runtime.listener.config", use_: "config", short: "Inspect or update listener configuration", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: false, handler: "", side_effect: false, outputs: &[], flags: &[] },
        CommandSpec { name: "runtime.listener.config.set", use_: "set", short: "Update listener configuration", long: "", aliases: &[], phase: "phase7", hidden: false, implemented: true, handler: "runtime.listener.config.set", side_effect: true, outputs: &["json", "pretty"], flags: &[flag!("enabled", "bool", "Enable or disable listener management"), flag!("auto-install", "bool", "Automatically install the listener service"), flag!("auto-start", "bool", "Automatically start the listener service")] },
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
