//! The installer and in-process upgrader accept the same bounded package layout.
//! Extract only regular files and the one documented runtime alias. Never pass
//! unvalidated archive paths, links or permissions to a system extractor.
use anyhow::{bail, ensure, Context, Result};
use flate2::read::GzDecoder;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

const MAX_FILES: usize = 50_000;
const MAX_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_METADATA: u64 = 2 * 1024 * 1024;
const ROOT_FILES: &[&str] = &[
    "awiki-deamon",
    "awiki-deamon-runtime",
    "README.txt",
    "LICENSE",
    "LICENSE-APACHE",
    "COMMERCIAL-LICENSING.md",
    "SOURCE.md",
    "checksums.txt",
];

fn path_name(raw: &str, directory: bool) -> Result<&str> {
    let mut name = raw;
    while name.starts_with("./") {
        name = &name[2..];
    }
    if directory {
        name = name.trim_end_matches('/');
    }
    ensure!(
        !name.is_empty()
            && name.len() <= 4096
            && !name.chars().any(|c| c.is_control() || c == '\\')
            && name
                .split('/')
                .all(|s| !s.is_empty() && s != "." && s != ".."),
        "unsafe daemon package path"
    );
    let component = name == "acp" || name.starts_with("acp/");
    ensure!(
        component || ROOT_FILES.contains(&name),
        "unexpected daemon package entry"
    );
    ensure!(
        !directory || component,
        "unexpected daemon package directory"
    );
    ensure!(name != "acp" || directory, "ACP root must be a directory");
    Ok(name)
}

fn mode(path: &Path, executable: bool) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            path,
            fs::Permissions::from_mode(if executable { 0o755 } else { 0o644 }),
        )?;
    }
    #[cfg(not(unix))]
    let _ = (path, executable);
    Ok(())
}

pub(super) fn extract_archive(archive_path: &Path, stage: &Path) -> Result<()> {
    ensure!(
        fs::symlink_metadata(stage)?.is_dir() && fs::read_dir(stage)?.next().is_none(),
        "daemon package stage must be an empty directory"
    );
    let mut archive = tar::Archive::new(GzDecoder::new(File::open(archive_path)?));
    let mut names = HashSet::new();
    let mut bytes = 0u64;
    let mut runtime_link = false;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let kind = entry.header().entry_type();
        let raw = entry.path_bytes();
        let name = path_name(std::str::from_utf8(&raw)?, kind.is_dir())?.to_owned();
        ensure!(
            names.insert(name.clone()) && names.len() <= MAX_FILES,
            "duplicate or excessive daemon package entries"
        );
        bytes = bytes
            .checked_add(entry.size())
            .context("daemon package size overflow")?;
        ensure!(bytes <= MAX_BYTES, "daemon package exceeds size budget");
        if name == "checksums.txt" || name == "acp/manifest.json" {
            ensure!(
                entry.size() <= MAX_METADATA,
                "daemon package metadata exceeds size budget"
            );
        }
        let target = stage.join(&name);
        if kind.is_dir() {
            fs::create_dir_all(target)?;
        } else if name == "awiki-deamon-runtime" && kind.is_symlink() {
            ensure!(
                entry.link_name_bytes().as_deref() == Some(b"awiki-deamon"),
                "unsupported runtime alias"
            );
            runtime_link = true;
        } else {
            ensure!(
                kind.is_file(),
                "daemon package links and special files are forbidden"
            );
            fs::create_dir_all(target.parent().context("package parent missing")?)?;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)?;
            let copied = std::io::copy(&mut entry, &mut file)?;
            ensure!(copied == entry.size(), "truncated daemon package entry");
            file.flush()?;
            mode(
                &target,
                matches!(
                    name.as_str(),
                    "awiki-deamon" | "awiki-deamon-runtime" | "acp/node"
                ),
            )?;
        }
    }
    if runtime_link {
        ensure!(
            fs::symlink_metadata(stage.join("awiki-deamon"))?.is_file(),
            "daemon binary missing"
        );
        #[cfg(unix)]
        std::os::unix::fs::symlink("awiki-deamon", stage.join("awiki-deamon-runtime"))?;
        #[cfg(not(unix))]
        {
            fs::copy(
                stage.join("awiki-deamon"),
                stage.join("awiki-deamon-runtime"),
            )?;
        }
    }
    validate_extracted_package(stage)
}

pub(super) fn validate_extracted_package(root: &Path) -> Result<()> {
    ensure!(
        fs::symlink_metadata(root)?.is_dir(),
        "invalid daemon package directory"
    );
    let mut pending = vec![root.to_path_buf()];
    let mut hashes = BTreeMap::new();
    let mut count = 0;
    let mut total = 0u64;
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            let metadata = fs::symlink_metadata(&path)?;
            let name = path
                .strip_prefix(root)?
                .to_str()
                .context("non UTF-8 package path")?;
            path_name(name, metadata.is_dir())?;
            count += 1;
            ensure!(count <= MAX_FILES, "daemon package entry budget exceeded");
            if metadata.is_dir() {
                pending.push(path);
                continue;
            }
            if metadata.file_type().is_symlink() {
                ensure!(
                    name == "awiki-deamon-runtime"
                        && fs::read_link(&path)? == Path::new("awiki-deamon")
                        && fs::symlink_metadata(root.join("awiki-deamon"))?.is_file(),
                    "unsafe daemon package link"
                );
            } else {
                ensure!(metadata.is_file(), "invalid daemon package file");
            }
            let size = fs::metadata(&path)?.len();
            total = total
                .checked_add(size)
                .context("daemon package size overflow")?;
            ensure!(total <= MAX_BYTES, "daemon package size budget exceeded");
            if matches!(name, "checksums.txt" | "acp/manifest.json") {
                ensure!(
                    size <= MAX_METADATA,
                    "daemon package metadata budget exceeded"
                );
            }
            if name != "checksums.txt" {
                let mut file = File::open(&path)?;
                let mut digest = Sha256::new();
                let mut buf = [0u8; 64 * 1024];
                loop {
                    let n = file.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    digest.update(&buf[..n]);
                }
                hashes.insert(name.to_owned(), format!("{:x}", digest.finalize()));
            }
        }
    }
    ensure!(
        ROOT_FILES
            .iter()
            .all(|name| *name == "checksums.txt" || hashes.contains_key(*name)),
        "daemon package is incomplete"
    );
    let mut expected = BTreeMap::new();
    for line in fs::read_to_string(root.join("checksums.txt"))?.lines() {
        let bytes = line.as_bytes();
        ensure!(
            bytes.len() > 66
                && bytes[..64].iter().all(u8::is_ascii_hexdigit)
                && bytes[64] == b' '
                && matches!(bytes[65], b' ' | b'*'),
            "invalid daemon package checksum"
        );
        ensure!(
            expected
                .insert(line[66..].to_owned(), line[..64].to_ascii_lowercase())
                .is_none(),
            "duplicate daemon package checksum"
        );
    }
    ensure!(
        expected == hashes,
        "daemon package checksums do not match complete contents"
    );
    if root.join("acp").exists() {
        validate_components(root, &hashes)?;
    }
    Ok(())
}

fn validate_components(root: &Path, hashes: &BTreeMap<String, String>) -> Result<()> {
    let manifest: Value = serde_json::from_slice(&fs::read(root.join("acp/manifest.json"))?)?;
    ensure!(
        manifest["schema_version"] == 1,
        "invalid ACP component manifest"
    );
    let files: BTreeMap<String, String> = hashes
        .iter()
        .filter_map(|(name, hash)| {
            name.strip_prefix("acp/")
                .filter(|n| *n != "manifest.json")
                .map(|n| (n.into(), hash.clone()))
        })
        .collect();
    ensure!(
        manifest["files"] == serde_json::to_value(&files)?,
        "ACP component checksums do not match"
    );
    if manifest["available"] == true {
        for name in ["node", "LICENSE.node", "package.json", "package-lock.json"] {
            ensure!(
                files.contains_key(name),
                "ACP component payload is incomplete"
            );
        }
        ensure!(
            manifest["package_lock_sha256"] == files["package-lock.json"],
            "ACP lock checksum mismatch"
        );
        let adapters = manifest["adapters"]
            .as_object()
            .context("ACP adapters missing")?;
        ensure!(adapters.len() == 2, "ACP adapter declarations incomplete");
        for brand in ["codex", "claude-code"] {
            let name = adapters
                .get(brand)
                .and_then(|v| v["entry"].as_str())
                .context("ACP adapter entry missing")?;
            ensure!(
                files.contains_key(name),
                "ACP adapter entry missing from package"
            );
        }
    } else if manifest["available"] != false || !files.is_empty() {
        bail!("invalid unsupported-platform ACP payload");
    }
    Ok(())
}

#[cfg(test)]
#[path = "package_tests.rs"]
mod tests;
