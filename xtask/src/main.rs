use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const MAX_LINES: usize = 1200;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("check-structure") => check_structure(),
        Some(other) => bail!("unknown xtask command {other:?}; expected check-structure"),
        None => bail!("missing xtask command; expected check-structure"),
    }
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
        if count > MAX_LINES && !exceptions.contains_key(&rel) {
            offenders.push((rel, count));
        }
        Ok(())
    })?;
    if offenders.is_empty() {
        println!("structure ok: no undocumented Rust files over {MAX_LINES} lines");
        return Ok(());
    }
    for (path, lines) in &offenders {
        eprintln!(
            "{path}: {lines} lines exceeds {MAX_LINES} without docs/file-size-exceptions.md entry"
        );
    }
    bail!(
        "structure check failed with {} undocumented oversized Rust files",
        offenders.len()
    )
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
