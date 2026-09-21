use super::*;
use flate2::{write::GzEncoder, Compression};
use std::collections::BTreeMap;

fn payload(components: bool) -> BTreeMap<String, Vec<u8>> {
    let mut files: BTreeMap<String, Vec<u8>> = ROOT_FILES
        .iter()
        .filter(|n| **n != "checksums.txt")
        .map(|name| ((*name).into(), name.as_bytes().to_vec()))
        .collect();
    if components {
        for name in [
            "node",
            "LICENSE.node",
            "package.json",
            "package-lock.json",
            "codex.js",
            "claude.js",
        ] {
            files.insert(format!("acp/{name}"), name.as_bytes().to_vec());
        }
        let hashes: BTreeMap<String, String> = files
            .iter()
            .filter_map(|(n, v)| {
                n.strip_prefix("acp/")
                    .map(|n| (n.to_owned(), format!("{:x}", Sha256::digest(v))))
            })
            .collect();
        let manifest = serde_json::json!({"schema_version":1,"available":true,
            "files":hashes,"package_lock_sha256":hashes["package-lock.json"],
            "adapters":{"codex":{"entry":"codex.js"},"claude-code":{"entry":"claude.js"}}});
        files.insert(
            "acp/manifest.json".into(),
            serde_json::to_vec(&manifest).unwrap(),
        );
    }
    checksums(&mut files);
    files
}

fn checksums(files: &mut BTreeMap<String, Vec<u8>>) {
    files.remove("checksums.txt");
    let contents: String = files
        .iter()
        .map(|(name, bytes)| format!("{:x}  {name}\n", Sha256::digest(bytes)))
        .collect();
    files.insert("checksums.txt".into(), contents.into_bytes());
}

fn archive(
    root: &Path,
    files: &BTreeMap<String, Vec<u8>>,
    extra: Option<(&str, tar::EntryType, &str)>,
) -> std::path::PathBuf {
    let path = root.join("package.tar.gz");
    let mut tar = tar::Builder::new(GzEncoder::new(
        File::create(&path).unwrap(),
        Compression::fast(),
    ));
    for (name, bytes) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o4777); // permissions from a tar must not be inherited
        header.set_cksum();
        tar.append_data(&mut header, name, bytes.as_slice())
            .unwrap();
    }
    if let Some((name, kind, target)) = extra {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(kind);
        header.set_size(0);
        header.set_mode(0o755);
        if kind.is_symlink() || kind.is_hard_link() {
            header.set_link_name(target).unwrap();
        }
        header.set_cksum();
        tar.append_data(&mut header, name, &[][..]).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap();
    path
}

#[test]
fn validates_legacy_and_component_packages_and_ignores_unsafe_modes() {
    for components in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let tar = archive(root.path(), &payload(components), None);
        let stage = root.path().join("stage");
        fs::create_dir(&stage).unwrap();
        extract_archive(&tar, &stage).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(stage.join("awiki-deamon"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o7777,
                0o755
            );
            if components {
                assert_eq!(
                    fs::metadata(stage.join("acp/node"))
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o7777,
                    0o755
                );
            }
        }
        fs::write(stage.join("awiki-deamon"), "corrupted").unwrap();
        assert!(validate_extracted_package(&stage).is_err());
    }
}

#[test]
fn rejects_duplicates_links_special_files_and_unlisted_payload() {
    for (name, kind, target) in [
        ("acp/evil", tar::EntryType::Symlink, "/tmp"),
        ("acp/evil", tar::EntryType::Link, "awiki-deamon"),
        ("acp/evil", tar::EntryType::Fifo, ""),
        ("awiki-deamon", tar::EntryType::Regular, ""),
        ("acp/unlisted", tar::EntryType::Regular, ""),
        ("unexpected", tar::EntryType::Regular, ""),
    ] {
        let root = tempfile::tempdir().unwrap();
        let tar = archive(root.path(), &payload(true), Some((name, kind, target)));
        let stage = root.path().join("stage");
        fs::create_dir(&stage).unwrap();
        assert!(
            extract_archive(&tar, &stage).is_err(),
            "accepted {name} {kind:?}"
        );
    }
}

#[test]
fn rejects_component_manifest_inconsistency_even_when_archive_checksums_match() {
    let root = tempfile::tempdir().unwrap();
    let mut files = payload(true);
    files.remove("acp/claude.js");
    checksums(&mut files);
    let tar = archive(root.path(), &files, None);
    let stage = root.path().join("stage");
    fs::create_dir(&stage).unwrap();
    assert!(extract_archive(&tar, &stage).is_err());
}

#[cfg(unix)]
#[test]
fn permits_only_runtime_alias_and_revalidates_existing_installation() {
    let root = tempfile::tempdir().unwrap();
    let mut files = payload(true);
    files.insert("awiki-deamon-runtime".into(), files["awiki-deamon"].clone());
    checksums(&mut files);
    files.remove("awiki-deamon-runtime");
    let tar = archive(
        root.path(),
        &files,
        Some((
            "awiki-deamon-runtime",
            tar::EntryType::Symlink,
            "awiki-deamon",
        )),
    );
    let stage = root.path().join("stage");
    fs::create_dir(&stage).unwrap();
    extract_archive(&tar, &stage).unwrap();
    fs::remove_file(stage.join("acp/node")).unwrap();
    std::os::unix::fs::symlink(stage.join("awiki-deamon"), stage.join("acp/node")).unwrap();
    assert!(validate_extracted_package(&stage).is_err());
}

#[test]
fn rejects_unsafe_paths_and_accepts_scoped_packages_with_spaces() {
    for path in [
        "../x", "/acp/x", "acp/../x", "acp//x", "acp/x\n", "acp/x\\y", "acp/./x",
    ] {
        assert!(path_name(path, false).is_err(), "accepted {path}");
    }
    assert_eq!(
        path_name("./acp/node_modules/@agent/client/some file.js", false).unwrap(),
        "acp/node_modules/@agent/client/some file.js"
    );
}
