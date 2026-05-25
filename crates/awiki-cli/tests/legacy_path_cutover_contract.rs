use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const BASELINE: &str = include_str!("../../../docs/sdk-refactor/legacy-path-baseline.md");
const SCAN_ROOTS: &[&str] = &[
    "crates/awiki-cli/src/app",
    "crates/awiki-cli/src/im_core_adapter",
    "crates/awiki-cli/src/runtime",
];
const NEEDLES: &[&str] = &[
    "should_fallback_attachment_send",
    "should_fallback_attachment_download",
    "legacy_attachment_send",
    "legacy_attachment_download",
    concat!("crate::", "message::"),
    concat!("im_core::", "compat::attachments::"),
    "store::store_message(",
    "store::list_dids_by_handle",
    concat!("im_core::", "compat::groups::raw_response"),
    "sync_group_state(",
    "cached_group_snapshot(",
    "cached_group_members(",
    "store::list_cached_group_members(",
    "enrich_cached_group_snapshot",
    concat!("im_core::", "compat::identity::"),
    "legacy_profile_value",
    concat!("use crate::", "message;"),
    "should_legacy_handle_raw_notification_with_im_core_runner",
    concat!("im_core::", "compat::realtime"),
    concat!("im_core::", "compat::wire"),
];

#[test]
fn final_cutover_legacy_path_baseline_matches_current_sources() {
    let baseline = parse_baseline(BASELINE);
    let actual = scan_sources(&workspace_root());

    assert_eq!(
        actual, baseline,
        "legacy path baseline drifted; update docs/sdk-refactor/legacy-path-baseline.md when burning down an offender, and do not add new offenders"
    );
}

#[test]
fn final_cutover_baseline_records_removal_workstream_for_each_offender() {
    for line in BASELINE.lines().filter(|line| line.starts_with("| ")) {
        if line.contains(" Area ") || line.contains(" --- ") {
            continue;
        }
        let columns = markdown_columns(line);
        assert_eq!(
            columns.len(),
            6,
            "baseline row should have 6 columns: {line}"
        );
        assert!(
            columns[5].starts_with('F'),
            "baseline row should name a final-cutover removal PR: {line}"
        );
        assert!(
            !columns[4].trim().is_empty(),
            "baseline row should explain why the offender remains: {line}"
        );
    }
}

fn parse_baseline(raw: &str) -> BTreeMap<OffenderKey, usize> {
    let mut result = BTreeMap::new();
    for line in raw.lines().filter(|line| line.starts_with("| ")) {
        if line.contains(" Area ") || line.contains(" --- ") {
            continue;
        }
        let columns = markdown_columns(line);
        assert_eq!(
            columns.len(),
            6,
            "baseline row should have 6 columns: {line}"
        );
        let file = columns[1].to_string();
        let needle = columns[2]
            .trim()
            .trim_start_matches('`')
            .trim_end_matches('`')
            .to_string();
        let count = columns[3]
            .parse::<usize>()
            .unwrap_or_else(|_| panic!("baseline count should be an integer: {line}"));
        assert!(
            NEEDLES.contains(&needle.as_str()),
            "baseline needle must be part of the static gate list: {needle}"
        );
        assert!(
            result.insert(OffenderKey { file, needle }, count).is_none(),
            "duplicate baseline offender row: {line}"
        );
    }
    result
}

fn scan_sources(root: &Path) -> BTreeMap<OffenderKey, usize> {
    let mut result = BTreeMap::new();
    for scan_root in SCAN_ROOTS {
        scan_dir(&root.join(scan_root), &mut |file, line| {
            for needle in NEEDLES {
                if line.contains(needle) {
                    let relative = file.strip_prefix(root).unwrap_or(file);
                    let key = OffenderKey {
                        file: slash_path(relative),
                        needle: (*needle).to_string(),
                    };
                    *result.entry(key).or_default() += 1;
                }
            }
        });
    }
    result
}

fn scan_dir(path: &Path, on_line: &mut dyn FnMut(&Path, &str)) {
    let mut entries = std::fs::read_dir(path)
        .unwrap_or_else(|err| panic!("read source directory {}: {err}", path.display()))
        .map(|entry| entry.expect("read source directory entry").path())
        .collect::<Vec<_>>();
    entries.sort();
    for entry in entries {
        if entry.is_dir() {
            scan_dir(&entry, on_line);
            continue;
        }
        if entry.extension().and_then(|value| value.to_str()) != Some("rs") {
            continue;
        }
        let content = std::fs::read_to_string(&entry)
            .unwrap_or_else(|err| panic!("read source file {}: {err}", entry.display()));
        for line in content.lines() {
            on_line(&entry, line);
        }
    }
}

fn markdown_columns(line: &str) -> Vec<&str> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .collect()
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn slash_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct OffenderKey {
    file: String,
    needle: String,
}
