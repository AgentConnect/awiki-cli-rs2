//! Exercise the shipped probe with Python import machinery, without Hermes or pip.
use super::*;

struct Installation {
    root: tempfile::TempDir,
}

impl Installation {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        for dir in ["site-packages", "source tree", "cwd"] {
            std::fs::create_dir(root.path().join(dir)).unwrap();
        }
        Self { root }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.path().join(relative)
    }

    fn package(&self, base: &str, entry: bool) {
        let package = self.path(base).join("tui_gateway");
        std::fs::create_dir(&package).unwrap();
        // Any accidental import (including util.find_spec on the child module)
        // must fail and leave evidence even if a future probe swallows errors.
        let guard = "from pathlib import Path\nPath(__file__).with_suffix('.imported').touch()\nraise RuntimeError('probe must not initialize Hermes')\n";
        std::fs::write(package.join("__init__.py"), guard).unwrap();
        if entry {
            std::fs::write(package.join("entry.py"), guard).unwrap();
        }
    }

    fn finder(&self, body: &str) {
        // Model a PEP 660 editable installation: .pth registers a MetaPath finder;
        // the source directory itself is absent from sys.path.
        std::fs::write(
            self.path("site-packages/hermes_fixture.pth"),
            "import hermes_fixture; hermes_fixture.install()\n",
        )
        .unwrap();
        std::fs::write(
            self.path("site-packages/hermes_fixture.py"),
            format!(
                "import sys, importlib.util\nfrom pathlib import Path\nclass Finder:\n    @classmethod\n    def find_spec(cls, fullname, path=None, target=None):\n        if fullname == 'tui_gateway':\n            {body}\n        return None\ndef install():\n    sys.meta_path.append(Finder)\n"
            ),
        )
        .unwrap();
    }

    fn editable(&self) {
        self.finder("return importlib.util.spec_from_file_location(fullname, Path(__file__).parent.parent / 'source tree' / 'tui_gateway' / '__init__.py')");
    }

    fn run(&self, deadline: Instant, extra_setup: &str) -> Result<String, &'static str> {
        let mut command = Command::new("python3");
        // -I/-S exclude user environment, installed packages and startup hooks.
        // Only the synthetic site's .pth is loaded; execute the production probe.
        let bootstrap = format!(
            "import site, sys\nsite.addsitedir(sys.argv[1])\n{extra_setup}\nexec(compile(sys.argv[2], '<hermes-probe>', 'exec'))\n"
        );
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", self.root.path())
            .current_dir(self.path("cwd"))
            .args(["-I", "-S", "-B", "-c", &bootstrap])
            .arg(self.path("site-packages"))
            .arg(HERMES_MODULE_PROBE);
        let result = process::run(&mut command, deadline);
        for base in ["site-packages", "source tree", "cwd"] {
            let package = self.path(base).join("tui_gateway");
            assert!(!package.join("__init__.imported").exists());
            assert!(!package.join("entry.imported").exists());
            assert!(!package.join("__pycache__").exists());
        }
        result
    }

    fn probe(&self) -> Result<String, &'static str> {
        self.run(Instant::now() + ITEM_TIMEOUT, "")
    }
}

#[test]
fn regular_installation_is_detected_without_importing_gateway() {
    let installation = Installation::new();
    installation.package("site-packages", true);
    assert_eq!(installation.probe(), Ok(String::new()));
}

#[test]
fn editable_installation_is_detected_without_importing_gateway() {
    let installation = Installation::new();
    installation.package("source tree", true);
    installation.editable();
    assert_eq!(installation.probe(), Ok(String::new()));
}

#[test]
fn namespace_package_is_detected_without_importing_entry() {
    let installation = Installation::new();
    installation.package("site-packages", true);
    std::fs::remove_file(installation.path("site-packages/tui_gateway/__init__.py")).unwrap();
    assert_eq!(installation.probe(), Ok(String::new()));
}

#[test]
fn absent_package_or_entry_is_not_reported_as_installed() {
    let missing = Installation::new();
    assert_eq!(missing.probe(), Err("version_failed"));
    for base in ["site-packages", "source tree"] {
        let installation = Installation::new();
        installation.package(base, false);
        if base == "source tree" {
            installation.editable();
        }
        assert_eq!(installation.probe(), Err("version_failed"));
    }
}

#[test]
fn working_directory_is_not_treated_as_an_installation() {
    let installation = Installation::new();
    installation.package("cwd", true);
    assert_eq!(
        installation.run(Instant::now() + ITEM_TIMEOUT, "sys.path[:0] = ['', '.']"),
        Err("version_failed")
    );
}

#[test]
fn broken_import_hook_fails_closed_without_exposing_output() {
    let installation = Installation::new();
    installation.finder("raise RuntimeError('private configuration')");
    assert_eq!(installation.probe(), Err("version_failed"));
}

#[test]
fn hanging_import_hook_respects_probe_deadline() {
    let installation = Installation::new();
    installation.finder("__import__('time').sleep(60)");
    let start = Instant::now();
    assert_eq!(
        installation.run(start + Duration::from_millis(300), ""),
        Err("timeout")
    );
    assert!(start.elapsed() < Duration::from_secs(3));
}
