use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::fmt;
use std::io::{self, Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Pretty,
    Ndjson,
    Table,
}

impl Format {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Pretty => "pretty",
            Self::Ndjson => "ndjson",
            Self::Table => "table",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatError {
    raw: String,
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unsupported format {:?}", self.raw)
    }
}

impl std::error::Error for FormatError {}

pub fn normalize_format(raw: &str) -> Result<Format, FormatError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "json" => Ok(Format::Json),
        "pretty" => Ok(Format::Pretty),
        "ndjson" => Ok(Format::Ndjson),
        "table" => Ok(Format::Table),
        _ => Err(FormatError {
            raw: raw.to_string(),
        }),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct IdentityMeta {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub did: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Meta {
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<IdentityMeta>,
    pub dry_run: bool,
    pub format: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SuccessEnvelope {
    pub ok: bool,
    pub command: String,
    #[serde(skip_serializing_if = "Value::is_null")]
    pub data: Value,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub summary: String,
    #[serde(rename = "_notice", skip_serializing_if = "Option::is_none")]
    pub notice: Option<Value>,
    pub meta: Meta,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorDetail {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub hint: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Value::is_null")]
    pub details: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorEnvelope {
    pub ok: bool,
    pub error: ErrorDetail,
    #[serde(rename = "_notice", skip_serializing_if = "Option::is_none")]
    pub notice: Option<Value>,
    pub meta: Meta,
}

#[derive(Debug, Clone)]
pub struct ExitError {
    pub exit_code: i32,
    pub detail: ErrorDetail,
}

impl ExitError {
    pub fn new(
        code: &str,
        exit_code: i32,
        message: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self {
            exit_code,
            detail: ErrorDetail {
                code: code.to_string(),
                message: message.into(),
                hint: hint.into(),
                retryable: false,
                details: Value::Null,
            },
        }
    }
}

impl fmt::Display for ExitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.detail.message)
    }
}

impl std::error::Error for ExitError {}

pub fn render_success<W: Write>(
    mut writer: W,
    format: Format,
    jq_expr: &str,
    envelope: &SuccessEnvelope,
) -> anyhow::Result<()> {
    render(&mut writer, format, jq_expr, envelope)
}

pub fn render_error<W: Write>(
    mut writer: W,
    format: Format,
    jq_expr: &str,
    envelope: &ErrorEnvelope,
) -> anyhow::Result<()> {
    render(&mut writer, format, jq_expr, envelope)
}

fn render<W: Write, T: Serialize>(
    writer: &mut W,
    format: Format,
    jq_expr: &str,
    envelope: &T,
) -> anyhow::Result<()> {
    let mut value = serde_json::to_value(envelope)?;
    if !jq_expr.trim().is_empty() {
        value = apply_jq_subset(&value, jq_expr)?;
    }
    write_value(writer, format, &value)
}

fn apply_jq_subset(value: &Value, expr: &str) -> anyhow::Result<Value> {
    let trimmed = expr.trim();
    if !trimmed.starts_with('.') {
        anyhow::bail!("invalid jq expression: expected expression to start with '.'");
    }
    if trimmed.contains('[') && !trimmed.contains(']') {
        anyhow::bail!("invalid jq expression: unclosed array index");
    }
    let mut current = value;
    let mut rest = trimmed.trim_start_matches('.');
    while !rest.is_empty() {
        let next_dot = rest.find('.');
        let segment = match next_dot {
            Some(index) => &rest[..index],
            None => rest,
        };
        current = apply_segment(current, segment)?;
        rest = match next_dot {
            Some(index) => &rest[index + 1..],
            None => "",
        };
    }
    Ok(current.clone())
}

fn apply_segment<'a>(value: &'a Value, segment: &str) -> anyhow::Result<&'a Value> {
    let (name, slice) = parse_segment(segment)?;
    let current = value
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("jq execution failed: missing field {name:?}"))?;
    if let Some((start, end)) = slice {
        let array = current.as_array().ok_or_else(|| {
            anyhow::anyhow!("jq execution failed: field {name:?} is not an array")
        })?;
        let end = end.min(array.len());
        let start = start.min(end);
        let sliced = Value::Array(array[start..end].to_vec());
        return Ok(Box::leak(Box::new(sliced)));
    }
    Ok(current)
}

fn parse_segment(segment: &str) -> anyhow::Result<(&str, Option<(usize, usize)>)> {
    if segment.is_empty() {
        anyhow::bail!("invalid jq expression: empty path segment");
    }
    let Some(open) = segment.find('[') else {
        return Ok((segment, None));
    };
    let close = segment
        .rfind(']')
        .ok_or_else(|| anyhow::anyhow!("invalid jq expression: unclosed array index"))?;
    if close != segment.len() - 1 {
        anyhow::bail!("invalid jq expression: unsupported array suffix");
    }
    let name = &segment[..open];
    let selector = &segment[open + 1..close];
    let (start_raw, end_raw) = selector
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("invalid jq expression: only slices are supported"))?;
    let start = if start_raw.trim().is_empty() {
        0
    } else {
        start_raw.trim().parse::<usize>()?
    };
    let end = if end_raw.trim().is_empty() {
        usize::MAX
    } else {
        end_raw.trim().parse::<usize>()?
    };
    Ok((name, Some((start, end))))
}

fn write_value<W: Write>(writer: &mut W, format: Format, value: &Value) -> anyhow::Result<()> {
    match format {
        Format::Json | Format::Pretty => write_json(writer, value, true),
        Format::Ndjson => write_ndjson(writer, value),
        Format::Table => write_table(writer, value),
    }
}

fn write_json<W: Write>(writer: &mut W, value: &Value, pretty: bool) -> anyhow::Result<()> {
    if pretty {
        writeln!(writer, "{}", serde_json::to_string_pretty(value)?)?;
    } else {
        writeln!(writer, "{}", serde_json::to_string(value)?)?;
    }
    Ok(())
}

fn write_ndjson<W: Write>(writer: &mut W, value: &Value) -> anyhow::Result<()> {
    if let Value::Array(rows) = value {
        for row in rows {
            write_json(writer, row, false)?;
        }
        return Ok(());
    }
    write_json(writer, value, false)
}

fn write_table<W: Write>(writer: &mut W, value: &Value) -> anyhow::Result<()> {
    let table_value = table_view_value(value);
    match table_value {
        Value::Array(rows) => write_table_rows(writer, rows),
        Value::Object(object) => write_table_object(writer, object),
        other => write_json(writer, other, true),
    }
}

fn table_view_value(value: &Value) -> &Value {
    let unwrapped = unwrap_table_envelope(value);
    if unwrapped.get("command").is_some() {
        return unwrapped;
    }
    preferred_table_rows(unwrapped).unwrap_or(unwrapped)
}

fn unwrap_table_envelope(value: &Value) -> &Value {
    if value.get("ok").and_then(Value::as_bool) == Some(true) {
        return value.get("data").unwrap_or(value);
    }
    if value.get("ok").and_then(Value::as_bool) == Some(false) {
        return value.get("error").unwrap_or(value);
    }
    value
}

fn preferred_table_rows(value: &Value) -> Option<&Value> {
    let keys = [
        "rows",
        "items",
        "messages",
        "members",
        "pages",
        "identities",
        "groups",
        "followers",
        "following",
        "checks",
        "commands",
    ];
    for key in keys {
        if value.get(key).is_some_and(Value::is_array) {
            return value.get(key);
        }
    }
    None
}

fn write_table_object<W: Write>(writer: &mut W, object: &Map<String, Value>) -> anyhow::Result<()> {
    let mut keys: Vec<_> = object.keys().collect();
    keys.sort();
    for key in keys {
        writeln!(
            writer,
            "{key}\t{}",
            table_cell(object.get(key).unwrap_or(&Value::Null))?
        )?;
    }
    Ok(())
}

fn write_table_rows<W: Write>(writer: &mut W, rows: &[Value]) -> anyhow::Result<()> {
    if rows.is_empty() {
        writeln!(writer, "No rows")?;
        return Ok(());
    }
    let mut columns = BTreeSet::new();
    for row in rows {
        let Some(object) = row.as_object() else {
            return write_json(writer, &Value::Array(rows.to_vec()), true);
        };
        columns.extend(object.keys().cloned());
    }
    let columns: Vec<_> = columns.into_iter().collect();
    writeln!(writer, "{}", columns.join("\t"))?;
    for row in rows {
        let object = row.as_object().expect("checked above");
        let cells = columns
            .iter()
            .map(|column| table_cell(object.get(column).unwrap_or(&Value::Null)))
            .collect::<Result<Vec<_>, _>>()?;
        writeln!(writer, "{}", cells.join("\t"))?;
    }
    Ok(())
}

fn table_cell(value: &Value) -> anyhow::Result<String> {
    let cell = match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        other => serde_json::to_string(other)?,
    };
    Ok(cell)
}

pub fn render_error_to_stderr(format: Format, jq_expr: &str, envelope: &ErrorEnvelope) -> i32 {
    match render_error(io::stderr(), format, jq_expr, envelope) {
        Ok(()) => 0,
        Err(_) => {
            let _ = writeln!(io::stderr(), "{}", envelope.error.message);
            1
        }
    }
}
