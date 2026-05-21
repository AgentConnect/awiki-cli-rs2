use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn im_core_src_does_not_reference_cli_boundary_types() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src_dir = manifest_dir.join("src");
    let forbidden = [
        concat!("Parsed", "Command"),
        concat!("Exit", "Error"),
        concat!("Global", "Options"),
        concat!("config", "::", "Resolved"),
        concat!("identity", "::", "Manager"),
        concat!("awiki", "_", "cli"),
        concat!("crate", "::", "app"),
        concat!("crate", "::", "cli"),
        concat!("Actor", "Context"),
        concat!("Identity", "Runtime", "Paths"),
        concat!("SQLite", " connection"),
        concat!("raw ", "serde_json", " payload"),
    ];

    let mut failures = Vec::new();
    for path in rust_sources(&src_dir) {
        let source = fs::read_to_string(&path).unwrap();
        for needle in forbidden {
            if source.contains(needle) {
                failures.push(format!("{} contains {needle}", path.display()));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "im-core boundary violations:\n{}",
        failures.join("\n")
    );
}

fn rust_sources(dir: &Path) -> Vec<PathBuf> {
    let mut sources = Vec::new();
    visit_rust_sources(dir, &mut sources);
    sources
}

fn visit_rust_sources(dir: &Path, sources: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            visit_rust_sources(&path, sources);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            sources.push(path);
        }
    }
}
