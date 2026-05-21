use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const MAX_LINES: usize = 2500;
const TEST_MAX_LINES: usize = 3000;

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
    let mut offenders = Vec::new();
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
        if count > max_lines && !exceptions.contains_key(&rel) {
            offenders.push((rel, count, max_lines));
        }
        Ok(())
    })?;
    if offenders.is_empty() {
        println!(
            "structure ok: no undocumented Rust files over policy limits ({MAX_LINES} source, {TEST_MAX_LINES} tests)"
        );
        return Ok(());
    }
    for (path, lines, max_lines) in &offenders {
        eprintln!(
            "{path}: {lines} lines exceeds {max_lines} without docs/file-size-exceptions.md entry"
        );
    }
    bail!(
        "structure check failed with {} undocumented oversized Rust files",
        offenders.len()
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

fn read_exceptions(path: &Path) -> Result<BTreeMap<String, String>> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut entries = BTreeMap::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') || trimmed.contains("Rust path") || trimmed.contains("---") {
            continue;
        }
        let cells: Vec<_> = trimmed
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if let Some(rust_path) = cells.first() {
            if !rust_path.is_empty() && rust_path.starts_with('`') && rust_path.ends_with('`') {
                entries.insert(rust_path.trim_matches('`').to_string(), trimmed.to_string());
            }
        }
    }
    Ok(entries)
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
