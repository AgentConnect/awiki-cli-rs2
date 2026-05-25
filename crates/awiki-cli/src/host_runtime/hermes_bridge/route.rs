use super::{
    default_notify_prompt, inspect_route, should_replace_notify_prompt, DEFAULT_DELIVER_TARGET,
    DEFAULT_WEBHOOK_PORT, DEFAULT_WEBHOOK_ROUTE_NAME,
};
use rand::RngCore;
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Default)]
pub struct EnsureRouteOptions {
    pub hermes_home: String,
    pub route_name: String,
    pub deliver: String,
    pub webhook_port: u32,
    pub prompt: String,
}

pub fn ensure_route(mut options: EnsureRouteOptions) -> anyhow::Result<super::RouteState> {
    if options.hermes_home.trim().is_empty() {
        options.hermes_home = super::resolve_hermes_home()?;
    } else {
        options.hermes_home = options.hermes_home.trim().to_string();
    }
    if options.route_name.trim().is_empty() {
        options.route_name = DEFAULT_WEBHOOK_ROUTE_NAME.to_string();
    } else {
        options.route_name = options.route_name.trim().to_string();
    }
    if options.deliver.trim().is_empty() {
        options.deliver = DEFAULT_DELIVER_TARGET.to_string();
    } else {
        options.deliver = options.deliver.trim().to_string();
    }
    if options.webhook_port == 0 {
        options.webhook_port = DEFAULT_WEBHOOK_PORT;
    }
    if options.prompt.trim().is_empty() {
        options.prompt = default_notify_prompt().to_string();
    }

    let config_path = Path::new(&options.hermes_home).join("config.yaml");
    let (raw, config_exists) = read_config_yaml(&config_path)?;
    let mut draft = HermesConfigDraft::from_raw(&raw, &options.route_name);
    draft.ensure(&options);
    write_yaml_file(&config_path, draft.render(&options.route_name).as_bytes())?;

    let mut state = inspect_route(&options.hermes_home, &options.route_name)?;
    state.config_exists = config_exists;
    Ok(state)
}

fn read_config_yaml(path: &Path) -> anyhow::Result<(String, bool)> {
    match fs::read_to_string(path) {
        Ok(raw) => Ok((raw, true)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok((String::new(), false)),
        Err(err) => Err(anyhow::anyhow!("read Hermes config.yaml: {err}")),
    }
}

#[derive(Debug, Clone, Default)]
struct HermesConfigDraft {
    top_level_items: Vec<String>,
    platform_items: Vec<String>,
    webhook_items: Vec<String>,
    extra_items: Vec<String>,
    other_routes: Vec<String>,
    route_items: Vec<String>,
    webhook_port: Option<u32>,
    route: RouteDraft,
}

#[derive(Debug, Clone, Default)]
struct RouteDraft {
    secret: String,
    events_block: Option<String>,
    prompt: String,
    skills: Vec<String>,
    deliver: String,
    deliver_extra: BTreeMap<String, String>,
}

impl HermesConfigDraft {
    fn from_raw(raw: &str, route_name: &str) -> Self {
        let fields = super::parse_yaml_scalar_fields(raw);
        let route_prefix = vec![
            "platforms".to_string(),
            "webhook".to_string(),
            "extra".to_string(),
            "routes".to_string(),
            route_name.to_string(),
        ];
        let mut draft = Self::default();
        draft.top_level_items = capture_child_items(raw, &[], &["platforms"]);
        draft.platform_items = capture_child_items(raw, &["platforms"], &["webhook"]);
        draft.webhook_items =
            capture_child_items(raw, &["platforms", "webhook"], &["enabled", "extra"]);
        draft.extra_items =
            capture_child_items(raw, &["platforms", "webhook", "extra"], &["port", "routes"]);
        draft.other_routes = capture_child_items(
            raw,
            &["platforms", "webhook", "extra", "routes"],
            &[route_name],
        );
        draft.route_items = capture_child_items(
            raw,
            &["platforms", "webhook", "extra", "routes", route_name],
            &[
                "secret",
                "events",
                "prompt",
                "skills",
                "deliver",
                "deliver_extra",
            ],
        );
        draft.webhook_port =
            positive_u32_field(&fields, &["platforms", "webhook", "extra", "port"]);
        draft.route.secret = string_field(&fields, &route_prefix, "secret");
        draft.route.events_block = extract_raw_field(raw, &route_prefix, "events");
        draft.route.prompt = extract_route_prompt(raw, &route_prefix)
            .or_else(|| scalar_field(&fields, &route_prefix, "prompt"))
            .filter(|value| !matches!(value.trim(), "|" | ">"))
            .unwrap_or_default();
        draft.route.skills = extract_sequence(raw, &route_prefix, "skills").unwrap_or_else(|| {
            scalar_field(&fields, &route_prefix, "skills")
                .map(|value| parse_inline_sequence(&value))
                .unwrap_or_default()
        });
        draft.route.deliver = string_field(&fields, &route_prefix, "deliver");
        let extra_prefix = route_prefix
            .iter()
            .cloned()
            .chain(std::iter::once("deliver_extra".to_string()))
            .collect::<Vec<_>>();
        for (path, value) in fields {
            if path.len() == extra_prefix.len() + 1 && path.starts_with(&extra_prefix) {
                draft
                    .route
                    .deliver_extra
                    .insert(path[extra_prefix.len()].clone(), value);
            }
        }
        draft
    }

    fn ensure(&mut self, options: &EnsureRouteOptions) {
        if self.webhook_port.unwrap_or_default() == 0 {
            self.webhook_port = Some(options.webhook_port);
        }
        if self.route.secret.trim().is_empty() {
            self.route.secret = generate_secret();
        } else {
            self.route.secret = self.route.secret.trim().to_string();
        }
        if self.route.events_block.is_none() {
            self.route.events_block = Some("          events: []\n".to_string());
        }
        if should_replace_notify_prompt(&self.route.prompt) {
            self.route.prompt = options.prompt.clone();
        }
        cleanup_legacy_notify_skill(&mut self.route.skills);
        self.route.deliver = options.deliver.clone();
        cleanup_deliver_extra(&mut self.route.deliver_extra);
    }

    fn render(&self, route_name: &str) -> String {
        let mut output = String::new();
        render_raw_items(&mut output, &self.top_level_items);
        output.push_str("platforms:\n");
        render_raw_items(&mut output, &self.platform_items);
        output.push_str("  webhook:\n");
        output.push_str("    enabled: true\n");
        render_raw_items(&mut output, &self.webhook_items);
        output.push_str("    extra:\n");
        output.push_str(&format!(
            "      port: {}\n",
            self.webhook_port.unwrap_or(DEFAULT_WEBHOOK_PORT)
        ));
        render_raw_items(&mut output, &self.extra_items);
        output.push_str("      routes:\n");
        render_raw_items(&mut output, &self.other_routes);
        output.push_str(&format!("        {}:\n", render_key(route_name)));
        output.push_str(&format!(
            "          secret: {}\n",
            render_scalar(&self.route.secret)
        ));
        if let Some(events) = &self.route.events_block {
            output.push_str(events);
            if !events.ends_with('\n') {
                output.push('\n');
            }
        }
        output.push_str("          prompt: |\n");
        render_block_scalar(&mut output, &self.route.prompt, "            ");
        if !self.route.skills.is_empty() {
            output.push_str("          skills: [");
            for (index, skill) in self.route.skills.iter().enumerate() {
                if index > 0 {
                    output.push_str(", ");
                }
                output.push('"');
                output.push_str(&escape_double_quoted(skill));
                output.push('"');
            }
            output.push_str("]\n");
        }
        output.push_str(&format!(
            "          deliver: {}\n",
            render_scalar(&self.route.deliver)
        ));
        if !self.route.deliver_extra.is_empty() {
            output.push_str("          deliver_extra:\n");
            for (key, value) in &self.route.deliver_extra {
                output.push_str(&format!(
                    "            {}: {}\n",
                    render_key(key),
                    render_scalar(value)
                ));
            }
        }
        render_raw_items(&mut output, &self.route_items);
        output
    }
}

fn positive_u32_field(fields: &BTreeMap<Vec<String>, String>, path: &[&str]) -> Option<u32> {
    fields
        .get(&path.iter().map(|part| part.to_string()).collect::<Vec<_>>())
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|value| *value > 0)
}

fn string_field(fields: &BTreeMap<Vec<String>, String>, prefix: &[String], leaf: &str) -> String {
    scalar_field(fields, prefix, leaf).unwrap_or_default()
}

fn scalar_field(
    fields: &BTreeMap<Vec<String>, String>,
    prefix: &[String],
    leaf: &str,
) -> Option<String> {
    let mut path = prefix.to_vec();
    path.push(leaf.to_string());
    fields.get(&path).cloned()
}

fn extract_raw_field(raw: &str, prefix: &[String], field: &str) -> Option<String> {
    let prefix = prefix.iter().map(String::as_str).collect::<Vec<_>>();
    capture_child_items(raw, &prefix, &[])
        .into_iter()
        .find(|item| raw_item_key(item).is_some_and(|key| key == field))
}

fn raw_item_key(item: &str) -> Option<&str> {
    item.lines()
        .find(|line| !line.trim().is_empty())
        .and_then(|line| line.trim_start().split_once(':'))
        .map(|(key, _)| key.trim())
}

fn cleanup_legacy_notify_skill(skills: &mut Vec<String>) {
    if skills.len() == 1 && skills[0].trim() == "notify" {
        skills.clear();
    }
}

fn cleanup_deliver_extra(extra: &mut BTreeMap<String, String>) {
    extra.remove("chat_id");
    extra.remove("thread_id");
    extra.remove("message_thread_id");
}

fn capture_child_items(raw: &str, parent: &[&str], exclude: &[&str]) -> Vec<String> {
    let lines: Vec<&str> = raw.lines().collect();
    let parent_path = parent
        .iter()
        .map(|part| part.to_string())
        .collect::<Vec<_>>();
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut items = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let without_comment = lines[index].split('#').next().unwrap_or("").trim_end();
        if without_comment.trim().is_empty() {
            index += 1;
            continue;
        }
        let indent = count_indent(without_comment);
        while stack.last().is_some_and(|(level, _)| *level >= indent) {
            stack.pop();
        }
        let trimmed = without_comment.trim_start();
        let Some((key, value)) = trimmed.split_once(':') else {
            index += 1;
            continue;
        };
        let key = key.trim();
        let path = stack.iter().map(|(_, key)| key.clone()).collect::<Vec<_>>();
        if path == parent_path && !exclude.iter().any(|candidate| *candidate == key) {
            let (block, next_index) = capture_raw_item(&lines, index, indent);
            items.push(block);
            index = next_index;
            continue;
        }
        if value.trim().is_empty() {
            stack.push((indent, key.to_string()));
        }
        index += 1;
    }
    items
}

fn capture_raw_item(lines: &[&str], start: usize, indent: usize) -> (String, usize) {
    let mut end = start + 1;
    while end < lines.len() {
        let line = lines[end];
        if line.trim().is_empty() {
            end += 1;
            continue;
        }
        if count_indent(line) <= indent {
            break;
        }
        end += 1;
    }
    let mut block = lines[start..end].join("\n");
    block.push('\n');
    (block, end)
}

fn render_raw_items(output: &mut String, items: &[String]) {
    for item in items {
        output.push_str(item);
        if !item.ends_with('\n') {
            output.push('\n');
        }
    }
}

fn generate_secret() -> String {
    let mut bytes = [0u8; 24];
    let mut rng = rand::rngs::OsRng;
    if rng.try_fill_bytes(&mut bytes).is_err() {
        return "awiki-hermes-secret".to_string();
    }
    hex_lower(&bytes)
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn extract_route_prompt(raw: &str, route_prefix: &[String]) -> Option<String> {
    extract_block_scalar(raw, route_prefix, "prompt")
}

fn extract_block_scalar(raw: &str, prefix: &[String], field: &str) -> Option<String> {
    let lines: Vec<&str> = raw.lines().collect();
    let mut stack: Vec<(usize, String)> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let without_comment = line.split('#').next().unwrap_or("").trim_end();
        if without_comment.trim().is_empty() {
            continue;
        }
        let indent = count_indent(without_comment);
        while stack.last().is_some_and(|(level, _)| *level >= indent) {
            stack.pop();
        }
        let trimmed = without_comment.trim_start();
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        let path = stack.iter().map(|(_, key)| key.clone()).collect::<Vec<_>>();
        if key == field && path == prefix && matches!(value.chars().next(), Some('|') | Some('>')) {
            return Some(capture_block(&lines[index + 1..], indent));
        }
        if value.is_empty() {
            stack.push((indent, key.to_string()));
        }
    }
    None
}

fn capture_block(lines: &[&str], parent_indent: usize) -> String {
    let mut captured = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            captured.push((*line).to_string());
            continue;
        }
        if count_indent(line) <= parent_indent {
            break;
        }
        captured.push((*line).to_string());
    }
    while captured.last().is_some_and(|line| line.trim().is_empty()) {
        captured.pop();
    }
    let strip_indent = captured
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| count_indent(line))
        .min()
        .unwrap_or(parent_indent + 2);
    let mut lines = Vec::with_capacity(captured.len());
    for line in captured {
        if line.trim().is_empty() {
            lines.push(String::new());
        } else {
            lines.push(line.chars().skip(strip_indent).collect());
        }
    }
    lines.join("\n")
}

fn extract_sequence(raw: &str, prefix: &[String], field: &str) -> Option<Vec<String>> {
    let lines: Vec<&str> = raw.lines().collect();
    let mut stack: Vec<(usize, String)> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let without_comment = line.split('#').next().unwrap_or("").trim_end();
        if without_comment.trim().is_empty() {
            continue;
        }
        let indent = count_indent(without_comment);
        while stack.last().is_some_and(|(level, _)| *level >= indent) {
            stack.pop();
        }
        let trimmed = without_comment.trim_start();
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        let path = stack.iter().map(|(_, key)| key.clone()).collect::<Vec<_>>();
        if key == field && path == prefix {
            if value.starts_with('[') {
                return Some(parse_inline_sequence(value));
            }
            if value.is_empty() {
                return Some(capture_sequence(&lines[index + 1..], indent));
            }
        }
        if value.is_empty() {
            stack.push((indent, key.to_string()));
        }
    }
    None
}

fn capture_sequence(lines: &[&str], parent_indent: usize) -> Vec<String> {
    let mut values = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let indent = count_indent(line);
        if indent <= parent_indent {
            break;
        }
        let trimmed = line.trim_start();
        let Some(value) = trimmed.strip_prefix("- ") else {
            continue;
        };
        values.push(strip_quotes(value.trim()).to_string());
    }
    values
}

fn parse_inline_sequence(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    let Some(inner) = trimmed
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
    else {
        return Vec::new();
    };
    let mut values = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    for ch in inner.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' && quote == Some('"') {
            escaped = true;
            continue;
        }
        match quote {
            Some(active) if ch == active => quote = None,
            Some(_) => current.push(ch),
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None if ch == ',' => {
                push_sequence_item(&mut values, &mut current);
            }
            None => current.push(ch),
        }
    }
    push_sequence_item(&mut values, &mut current);
    values
}

fn push_sequence_item(values: &mut Vec<String>, current: &mut String) {
    let value = strip_quotes(current.trim());
    if !value.is_empty() {
        values.push(value.to_string());
    }
    current.clear();
}

fn strip_quotes(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value)
}

fn count_indent(line: &str) -> usize {
    line.chars().take_while(|ch| *ch == ' ').count()
}

fn render_block_scalar(output: &mut String, value: &str, indent: &str) {
    let value = value.trim_matches('\n');
    if value.is_empty() {
        output.push_str(indent);
        output.push('\n');
        return;
    }
    for line in value.lines() {
        output.push_str(indent);
        output.push_str(line);
        output.push('\n');
    }
}

fn render_key(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        value.to_string()
    } else {
        format!("\"{}\"", escape_double_quoted(value))
    }
}

fn render_scalar(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return "\"\"".to_string();
    }
    if value == "[]" || value == "{}" || value == "true" || value == "false" {
        return value.to_string();
    }
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | '@' | ':'))
    {
        value.to_string()
    } else {
        format!("\"{}\"", escape_double_quoted(value))
    }
}

fn escape_double_quoted(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn write_yaml_file(path: &Path, content: &[u8]) -> anyhow::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    create_config_dir(parent)?;
    let (mut temp_file, temp_path) = create_temp_config_file(parent)?;
    let mut cleanup = TempCleanup::new(temp_path.clone());
    temp_file
        .write_all(content)
        .map_err(|err| anyhow::anyhow!("write temp Hermes config file: {err}"))?;
    temp_file
        .sync_all()
        .map_err(|err| anyhow::anyhow!("sync temp Hermes config file: {err}"))?;
    drop(temp_file);
    set_file_mode(&temp_path, 0o600)?;
    fs::rename(&temp_path, path)
        .map_err(|err| anyhow::anyhow!("replace Hermes config file: {err}"))?;
    cleanup.keep();
    Ok(())
}

fn create_config_dir(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        fs::create_dir_all(path)
            .map_err(|err| anyhow::anyhow!("create Hermes config dir: {err}"))?;
        set_dir_mode(path, 0o700)?;
    }
    Ok(())
}

fn create_temp_config_file(parent: &Path) -> anyhow::Result<(File, PathBuf)> {
    for attempt in 0..100 {
        let path = parent.join(temp_config_name(attempt));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((file, path)),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(anyhow::anyhow!("create temp Hermes config file: {err}")),
        }
    }
    anyhow::bail!("create temp Hermes config file: too many temporary name collisions")
}

fn temp_config_name(attempt: u32) -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        ".hermes-config-{}-{nonce}-{attempt}.tmp",
        std::process::id()
    )
}

#[cfg(unix)]
fn set_dir_mode(path: &Path, mode: u32) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|err| anyhow::anyhow!("chmod Hermes config dir: {err}"))
}

#[cfg(not(unix))]
fn set_dir_mode(_path: &Path, _mode: u32) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_file_mode(path: &Path, mode: u32) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|err| anyhow::anyhow!("chmod temp Hermes config file: {err}"))
}

#[cfg(not(unix))]
fn set_file_mode(_path: &Path, _mode: u32) -> anyhow::Result<()> {
    Ok(())
}

struct TempCleanup {
    path: PathBuf,
    cleanup: bool,
}

impl TempCleanup {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            cleanup: true,
        }
    }

    fn keep(&mut self) {
        self.cleanup = false;
    }
}

impl Drop for TempCleanup {
    fn drop(&mut self) {
        if self.cleanup {
            let _ = fs::remove_file(&self.path);
        }
    }
}
