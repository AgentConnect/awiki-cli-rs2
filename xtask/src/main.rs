use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use time::{Date, Month, OffsetDateTime};

const MAX_LINES: usize = 2500;
const TEST_MAX_LINES: usize = 3000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileKind {
    Source,
    Test,
    Generated,
}

impl FileKind {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "Source" => Ok(Self::Source),
            "Test" => Ok(Self::Test),
            "Generated" => Ok(Self::Generated),
            _ => bail!("unknown exception kind {value:?}; expected Source, Test, or Generated"),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Source => "Source",
            Self::Test => "Test",
            Self::Generated => "Generated",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileSizeException {
    kind: FileKind,
    approved_lines: usize,
    owner: String,
    review_by: Date,
    reason: String,
    exit_condition: String,
}

#[derive(Debug, Clone, Copy)]
struct RustFileSize {
    lines: usize,
    policy_limit: usize,
    kind: FileKind,
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("check-structure") => check_structure(),
        Some("check-version") => check_version(args.collect()),
        Some(other) => {
            bail!("unknown xtask command {other:?}; expected check-structure or check-version")
        }
        None => bail!("missing xtask command; expected check-structure or check-version"),
    }
}

fn check_version(args: Vec<String>) -> Result<()> {
    let expected_version = parse_expected_version(args)?;
    let root = std::env::current_dir().context("read current dir")?;
    let package = read_package_json(&root.join("package.json"))?;
    let package_version =
        json_string(&package, &["version"]).context("package.json version must be a string")?;
    let min_supported_version = json_string(&package, &["awikiCli", "minSupportedVersion"])
        .context("package.json awikiCli.minSupportedVersion must be a string")?;
    let crate_version = read_cargo_package_version(&root.join("crates/awiki-cli/Cargo.toml"))?;

    ensure_semverish(&package_version).context("package.json version")?;
    ensure_semverish(&min_supported_version)
        .context("package.json awikiCli.minSupportedVersion")?;
    ensure_semverish(&crate_version).context("crates/awiki-cli/Cargo.toml package.version")?;

    if package_version != min_supported_version {
        bail!(
            "version mismatch: package.json version {package_version:?} != awikiCli.minSupportedVersion {min_supported_version:?}"
        );
    }
    if package_version != crate_version {
        bail!(
            "version mismatch: package.json version {package_version:?} != awiki-cli crate version {crate_version:?}"
        );
    }
    if let Some(expected_version) = expected_version {
        ensure_semverish(&expected_version).context("expected release/build version")?;
        if package_version != expected_version {
            bail!(
                "version mismatch: expected release/build version {expected_version:?} != package.json version {package_version:?}"
            );
        }
    }

    println!("version ok: package.json, awikiCli.minSupportedVersion, and awiki-cli crate are {package_version}");
    Ok(())
}

fn parse_expected_version(args: Vec<String>) -> Result<Option<String>> {
    let mut expected_version = None;
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--expect" | "--expected-version" => {
                let value = iter
                    .next()
                    .with_context(|| format!("{arg} requires a version value"))?;
                expected_version = Some(normalize_version(&value)?);
            }
            "--help" | "-h" => {
                println!(
                    "Usage: cargo run -p xtask -- check-version [--expect VERSION]\n\nChecks package.json.version, package.json.awikiCli.minSupportedVersion, and crates/awiki-cli/Cargo.toml package.version."
                );
                std::process::exit(0);
            }
            other => bail!("unknown check-version argument {other:?}; expected --expect VERSION"),
        }
    }
    Ok(expected_version)
}

fn normalize_version(raw: &str) -> Result<String> {
    let version = raw.trim().trim_start_matches('v').trim();
    if version.is_empty() {
        bail!("version must not be empty");
    }
    Ok(version.to_string())
}

fn read_package_json(path: &Path) -> Result<Value> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))
}

fn json_string(value: &Value, path: &[&str]) -> Option<String> {
    let mut cursor = value;
    for key in path {
        cursor = cursor.get(*key)?;
    }
    cursor
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn read_cargo_package_version(path: &Path) -> Result<String> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut in_package = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            continue;
        }
        if !in_package || !trimmed.starts_with("version") {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        if key.trim() != "version" {
            continue;
        }
        let version = value.trim().trim_matches('"').trim();
        if !version.is_empty() {
            return Ok(version.to_string());
        }
    }
    bail!("missing [package] version in {}", path.display())
}

fn ensure_semverish(version: &str) -> Result<()> {
    let (core, prerelease) = version.split_once('-').unwrap_or((version, ""));
    let mut parts = core.split('.');
    let Some(major) = parts.next() else {
        bail!("version must look like X.Y.Z, got {version:?}");
    };
    let Some(minor) = parts.next() else {
        bail!("version must look like X.Y.Z, got {version:?}");
    };
    let Some(patch) = parts.next() else {
        bail!("version must look like X.Y.Z, got {version:?}");
    };
    if parts.next().is_some()
        || !is_numeric_identifier(major)
        || !is_numeric_identifier(minor)
        || !is_numeric_identifier(patch)
    {
        bail!("version must look like X.Y.Z, got {version:?}");
    }
    if !prerelease.is_empty()
        && !prerelease.split('.').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
        })
    {
        bail!("pre-release version has invalid identifiers: {version:?}");
    }
    Ok(())
}

fn is_numeric_identifier(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|ch| ch.is_ascii_digit())
}

fn check_structure() -> Result<()> {
    let root = std::env::current_dir().context("read current dir")?;
    let exceptions = read_exceptions(&root.join("docs/file-size-exceptions.md"))?;
    let today = OffsetDateTime::now_utc().date();
    let mut files = BTreeMap::new();
    visit(&root, &mut |path| {
        if !is_counted_rust(path) {
            return Ok(());
        }
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let count = count_lines(path)?;
        let max_lines = max_lines_for_path(&rel);
        let kind = if is_generated_rust(path)? {
            FileKind::Generated
        } else if is_test_rust_path(&rel) {
            FileKind::Test
        } else {
            FileKind::Source
        };
        files.insert(
            rel,
            RustFileSize {
                lines: count,
                policy_limit: max_lines,
                kind,
            },
        );
        Ok(())
    })?;

    let mut violations = Vec::new();
    for (path, size) in &files {
        if size.lines <= size.policy_limit {
            if exceptions.contains_key(path) {
                violations.push(format!(
                    "{path}: {0} lines is within the {1}-line policy; remove its stale exception",
                    size.lines, size.policy_limit
                ));
            }
            continue;
        }

        let Some(exception) = exceptions.get(path) else {
            violations.push(format!(
                "{path}: {0} lines exceeds {1} without docs/file-size-exceptions.md entry",
                size.lines, size.policy_limit
            ));
            continue;
        };
        if exception.kind != size.kind {
            violations.push(format!(
                "{path}: exception kind {} does not match detected kind {}",
                exception.kind.label(),
                size.kind.label()
            ));
        }
        if size.lines > exception.approved_lines {
            violations.push(format!(
                "{path}: {0} lines exceeds its approved exception ceiling of {1}",
                size.lines, exception.approved_lines
            ));
        }
        if exception.review_by < today {
            violations.push(format!(
                "{path}: exception review date {} has expired; review, split, or renew it",
                exception.review_by
            ));
        }
    }
    for path in exceptions.keys() {
        if !files.contains_key(path) {
            violations.push(format!(
                "{path}: exception does not refer to a counted Rust file"
            ));
        }
    }

    if violations.is_empty() {
        println!(
            "structure ok: all Rust files meet policy or have current, bounded exceptions ({MAX_LINES} source, {TEST_MAX_LINES} tests)"
        );
        return Ok(());
    }
    for violation in &violations {
        eprintln!("{violation}");
    }
    bail!(
        "structure check failed with {} file-size policy violations",
        violations.len()
    )
}

fn max_lines_for_path(rel: &str) -> usize {
    if is_test_rust_path(rel) {
        TEST_MAX_LINES
    } else {
        MAX_LINES
    }
}

fn is_test_rust_path(rel: &str) -> bool {
    rel.contains("/tests/") || rel.ends_with("/tests.rs")
}

fn read_exceptions(path: &Path) -> Result<BTreeMap<String, FileSizeException>> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    parse_exceptions(&text, &path.display().to_string())
}

fn parse_exceptions(text: &str, source: &str) -> Result<BTreeMap<String, FileSizeException>> {
    let mut entries = BTreeMap::new();
    let mut in_table = false;
    for (line_index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("| Rust path |") {
            in_table = true;
            continue;
        }
        if !in_table || !trimmed.starts_with('|') {
            continue;
        }
        let cells: Vec<_> = trimmed
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if cells.first().is_some_and(|cell| cell.starts_with("---")) {
            continue;
        }
        if cells.len() != 7 {
            bail!(
                "{source}:{}: expected 7 exception columns, found {}",
                line_index + 1,
                cells.len()
            );
        }
        let rust_path = cells[0];
        if !rust_path.starts_with('`') || !rust_path.ends_with('`') || rust_path.len() < 3 {
            bail!(
                "{source}:{}: Rust path must be a non-empty backticked path",
                line_index + 1
            );
        }
        let rust_path = rust_path.trim_matches('`').to_string();
        let exception = FileSizeException {
            kind: FileKind::parse(cells[1])
                .with_context(|| format!("{source}:{}: invalid kind", line_index + 1))?,
            approved_lines: cells[2].parse::<usize>().with_context(|| {
                format!(
                    "{source}:{}: approved lines must be an integer",
                    line_index + 1
                )
            })?,
            owner: required_exception_cell(source, line_index, "owner", cells[3])?,
            review_by: parse_review_date(cells[4])
                .with_context(|| format!("{source}:{}: invalid review date", line_index + 1))?,
            reason: required_exception_cell(source, line_index, "reason", cells[5])?,
            exit_condition: required_exception_cell(
                source,
                line_index,
                "exit condition",
                cells[6],
            )?,
        };
        if exception.approved_lines == 0 {
            bail!(
                "{source}:{}: approved lines must be greater than zero",
                line_index + 1
            );
        }
        if entries.insert(rust_path.clone(), exception).is_some() {
            bail!(
                "{source}:{}: duplicate exception for {rust_path}",
                line_index + 1
            );
        }
    }
    Ok(entries)
}

fn required_exception_cell(
    source: &str,
    line_index: usize,
    name: &str,
    value: &str,
) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || value == "-" {
        bail!("{source}:{}: {name} must not be empty", line_index + 1);
    }
    Ok(value.to_string())
}

fn parse_review_date(value: &str) -> Result<Date> {
    let parts = value
        .split('-')
        .map(str::parse::<i32>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("review date must use YYYY-MM-DD, got {value:?}"))?;
    if parts.len() != 3 || parts[0] < 0 {
        bail!("review date must use YYYY-MM-DD, got {value:?}");
    }
    let month = u8::try_from(parts[1])
        .ok()
        .and_then(|month| Month::try_from(month).ok())
        .with_context(|| format!("invalid month in review date {value:?}"))?;
    let day =
        u8::try_from(parts[2]).with_context(|| format!("invalid day in review date {value:?}"))?;
    Date::from_calendar_date(parts[0], month, day)
        .with_context(|| format!("invalid review date {value:?}"))
}

fn visit(dir: &Path, f: &mut dyn FnMut(&Path) -> Result<()>) -> Result<()> {
    if should_skip_dir(dir) {
        return Ok(());
    }
    for entry in fs::read_dir(dir).with_context(|| format!("read dir {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            visit(&path, f)?;
        } else {
            f(&path)?;
        }
    }
    Ok(())
}

fn should_skip_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, ".git" | "target" | "vendor"))
}

fn is_counted_rust(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("rs")
        && !path.components().any(|component| {
            let text = component.as_os_str().to_string_lossy();
            matches!(text.as_ref(), "target" | "vendor")
        })
}

fn count_lines(path: &Path) -> Result<usize> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    Ok(text.lines().count())
}

fn is_generated_rust(path: &Path) -> Result<bool> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    Ok(text
        .lines()
        .take(8)
        .any(|line| line.contains("@generated") || line.contains("automatically generated")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "| Rust path | Kind | Approved lines | Owner | Review by | Reason | Exit condition |\n| --- | --- | ---: | --- | --- | --- | --- |\n";

    #[test]
    fn exception_table_requires_complete_governance_metadata() {
        let text = format!(
            "{HEADER}| `crates/example/src/lib.rs` | Source | 2600 | Example team | 2099-12-31 | Cohesive migration state machine. | Split transport from persistence after migration. |\n"
        );
        let entries = parse_exceptions(&text, "test table").expect("valid table");
        let entry = entries
            .get("crates/example/src/lib.rs")
            .expect("parsed exception");
        assert_eq!(entry.kind, FileKind::Source);
        assert_eq!(entry.approved_lines, 2600);
        assert_eq!(entry.owner, "Example team");
        assert_eq!(entry.review_by.to_string(), "2099-12-31");
    }

    #[test]
    fn exception_table_rejects_missing_exit_condition() {
        let text = format!(
            "{HEADER}| `crates/example/src/lib.rs` | Source | 2600 | Example team | 2099-12-31 | Temporary migration. | - |\n"
        );
        let error = parse_exceptions(&text, "test table").expect_err("missing exit condition");
        assert!(error
            .to_string()
            .contains("exit condition must not be empty"));
    }

    #[test]
    fn exception_table_rejects_duplicate_paths() {
        let row = "| `crates/example/src/lib.rs` | Source | 2600 | Example team | 2099-12-31 | Temporary migration. | Split after migration. |\n";
        let error = parse_exceptions(&format!("{HEADER}{row}{row}"), "test table")
            .expect_err("duplicate path");
        assert!(error.to_string().contains("duplicate exception"));
    }
}
