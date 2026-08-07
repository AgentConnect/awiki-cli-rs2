import contextlib
import hashlib
import http.server
import json
import os
import pathlib
import platform
import shutil
import socketserver
import subprocess
import tempfile
import threading
import unittest
from urllib.parse import quote


ROOT = pathlib.Path(__file__).resolve().parents[1]
TARGETS = (
    ("darwin", "amd64"),
    ("darwin", "arm64"),
    ("linux", "amd64"),
)


def run_command(args, *, cwd=ROOT, env=None):
    command_env = {**os.environ, "COPYFILE_DISABLE": "1"}
    if env is not None:
        command_env.update(env)
    return subprocess.run(
        args,
        cwd=cwd,
        env=command_env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
    )


def current_installer_target() -> tuple[str, str]:
    system = platform.system()
    machine = platform.machine()
    if system == "Darwin":
        os_name = "darwin"
    elif system == "Linux":
        os_name = "linux"
    else:
        raise unittest.SkipTest(f"unsupported installer test OS: {system}")
    if machine in ("x86_64", "amd64"):
        arch = "amd64"
    elif machine in ("arm64", "aarch64"):
        arch = "arm64"
    else:
        raise unittest.SkipTest(f"unsupported installer test arch: {machine}")
    return os_name, arch


def create_fake_daemon_package(source_dir: pathlib.Path, os_name: str, arch: str) -> pathlib.Path:
    stage = source_dir / f"stage-{os_name}-{arch}"
    stage.mkdir(parents=True)
    daemon = stage / "awiki-deamon"
    daemon.write_text(
        "#!/bin/sh\n"
        "printf '%s\\n' \"$@\" > \"$HOME/fake-awiki-deamon-args.txt\"\n"
        "printf 'fake daemon invoked\\n'\n",
        encoding="utf-8",
    )
    daemon.chmod(0o755)
    (stage / "awiki-deamon-runtime").symlink_to("awiki-deamon")
    (stage / "README.txt").write_text("fake daemon package\n", encoding="utf-8")
    (stage / "LICENSE").write_text("fake license\n", encoding="utf-8")
    (stage / "LICENSE-APACHE").write_text("fake Apache license\n", encoding="utf-8")
    (stage / "COMMERCIAL-LICENSING.md").write_text(
        "fake commercial licensing policy\n", encoding="utf-8"
    )
    (stage / "SOURCE.md").write_text("Commit: fake-commit\n", encoding="utf-8")
    (stage / "checksums.txt").write_text("fake inner checksums\n", encoding="utf-8")

    archive = source_dir / f"awiki-deamon-{os_name}-{arch}.tar.gz"
    run_command(
        [
            "tar",
            "-C",
            str(stage),
            "-czf",
            str(archive),
            "awiki-deamon",
            "awiki-deamon-runtime",
            "README.txt",
            "LICENSE",
            "LICENSE-APACHE",
            "COMMERCIAL-LICENSING.md",
            "SOURCE.md",
            "checksums.txt",
        ]
    )
    return archive


def create_all_fake_packages(source_dir: pathlib.Path) -> None:
    source_dir.mkdir(parents=True, exist_ok=True)
    for os_name, arch in TARGETS:
        create_fake_daemon_package(source_dir, os_name, arch)


class QuietHttpServer(socketserver.ThreadingTCPServer):
    allow_reuse_address = True


class QuietRequestHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, format, *args):  # noqa: A002
        return


@contextlib.contextmanager
def serve_directory(directory: pathlib.Path):
    handler = lambda *args, **kwargs: QuietRequestHandler(  # noqa: E731
        *args,
        directory=str(directory),
        **kwargs,
    )
    with QuietHttpServer(("127.0.0.1", 0), handler) as server:
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            yield f"http://127.0.0.1:{server.server_address[1]}"
        finally:
            server.shutdown()
            thread.join(timeout=5)


def file_url(path: pathlib.Path) -> str:
    return "file://" + quote(str(path.resolve()))


def readlink(path: pathlib.Path) -> str:
    if hasattr(path, "readlink"):
        return str(path.readlink())
    return subprocess.check_output(["readlink", str(path)], text=True).strip()


class DaemonReleaseContractTests(unittest.TestCase):
    def test_stage_download_layout_uses_path_only_manifest_and_embeds_download_sources(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            temp_dir = pathlib.Path(temp)
            source_dir = temp_dir / "source"
            create_all_fake_packages(source_dir)

            output_dir = temp_dir / "daemon"
            download_base_url = "https://example.com/daemon"
            mirror_url = "https://download.example.com/daemon"
            run_command(
                [
                    "scripts/release/daemon/_stage-downloads.sh",
                    "--version",
                    "v1.2.3",
                    "--source-dir",
                    str(source_dir),
                    "--output-dir",
                    str(output_dir),
                    "--base-url",
                    "https://api.example.com",
                    "--download-base-url",
                    download_base_url,
                    "--download-mirror-url",
                    mirror_url,
                    "--min-supported",
                    "1.0.0",
                ]
            )

            install_text = (output_dir / "install.sh").read_text(encoding="utf-8")
            self.assertIn("https://api.example.com", install_text)
            self.assertIn("DEFAULT_DOWNLOAD_BASE_URLS='https://example.com/daemon", install_text)
            self.assertIn("https://download.example.com/daemon", install_text)
            self.assertIn("SELECTED_DOWNLOAD_BASE_URL", install_text)
            self.assertIn("--progress-bar", install_text)
            self.assertIn("--speed-limit \"$CURL_SPEED_LIMIT_BYTES\"", install_text)
            cleanup_text = (output_dir / "cleanup.sh").read_text(encoding="utf-8")
            self.assertIn("permanently remove all AWiki daemon data", cleanup_text)
            self.assertIn("$HOME/.awiki-daemon/deamon", cleanup_text)
            self.assertIn("Type CLEANUP to continue", cleanup_text)
            self.assertIn("launchctl bootout", cleanup_text)
            self.assertIn("systemctl --user stop awiki-deamon.service", cleanup_text)

            manifest = json.loads(
                (output_dir / "releases" / "manifest.json").read_text(encoding="utf-8")
            )
            self.assertEqual(manifest["latest"], "1.2.3")
            self.assertEqual(manifest["min_supported"], "1.0.0")
            self.assertEqual(len(manifest["packages"]), len(TARGETS))

            packages_by_target = {
                (package["os"], package["arch"]): package for package in manifest["packages"]
            }
            for os_name, arch in TARGETS:
                package_name = f"awiki-deamon-{os_name}-{arch}.tar.gz"
                package_path = output_dir / "releases" / "1.2.3" / package_name
                self.assertTrue(package_path.is_file())
                package = packages_by_target[(os_name, arch)]
                self.assertEqual(package["version"], "1.2.3")
                self.assertNotIn("url", package)
                self.assertEqual(package["path"], f"releases/1.2.3/{package_name}")
                expected_sha = hashlib.sha256(package_path.read_bytes()).hexdigest()
                self.assertEqual(package["sha256"], expected_sha)

    def test_stage_downloads_resolves_relative_paths_from_caller_directory(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            temp_dir = pathlib.Path(temp)
            caller_dir = temp_dir / "caller"
            caller_dir.mkdir()
            source_dir = caller_dir / "dist"
            create_all_fake_packages(source_dir)

            run_command(
                [
                    str(ROOT / "scripts/release/daemon/_stage-downloads.sh"),
                    "--version",
                    "1.2.3",
                    "--source-dir",
                    "dist",
                    "--output-dir",
                    "download-root",
                    "--base-url",
                    "https://awiki.ai",
                ],
                cwd=caller_dir,
            )

            output_dir = caller_dir / "download-root"
            self.assertTrue((output_dir / "install.sh").is_file())
            manifest = json.loads(
                (output_dir / "releases" / "manifest.json").read_text(encoding="utf-8")
            )
            self.assertEqual(manifest["latest"], "1.2.3")
            self.assertTrue(
                (
                    output_dir
                    / "releases"
                    / "1.2.3"
                    / "awiki-deamon-darwin-arm64.tar.gz"
                ).is_file()
            )

    def test_generate_manifest_requires_all_supported_packages(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            temp_dir = pathlib.Path(temp)
            for os_name, arch in TARGETS[:-1]:
                create_fake_daemon_package(temp_dir, os_name, arch)

            result = subprocess.run(
                [
                    "node",
                    "scripts/release/daemon/_generate-manifest.js",
                    "--version",
                    "1.2.3",
                    "--dist",
                    str(temp_dir),
                    "--output",
                    str(temp_dir / "manifest.json"),
                ],
                cwd=ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("missing daemon package", result.stderr)

    def test_generate_manifest_allows_partial_existing_packages_without_base_url(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            temp_dir = pathlib.Path(temp)
            package_path = create_fake_daemon_package(temp_dir, "linux", "amd64")

            run_command(
                [
                    "node",
                    "scripts/release/daemon/_generate-manifest.js",
                    "--version",
                    "1.2.3",
                    "--dist",
                    str(temp_dir),
                    "--output",
                    str(temp_dir / "manifest.json"),
                    "--allow-partial",
                ]
            )

            manifest = json.loads((temp_dir / "manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(
                manifest["packages"],
                [
                    {
                        "version": "1.2.3",
                        "os": "linux",
                        "arch": "amd64",
                        "path": "releases/1.2.3/awiki-deamon-linux-amd64.tar.gz",
                        "sha256": hashlib.sha256(package_path.read_bytes()).hexdigest(),
                    }
                ],
            )

    def test_generate_manifest_records_download_base_urls(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            temp_dir = pathlib.Path(temp)
            create_fake_daemon_package(temp_dir, "linux", "amd64")
            urls_file = temp_dir / "download-urls.txt"
            urls_file.write_text(
                "https://example.com/daemon\nhttps://cdn.example.com/daemon/\n",
                encoding="utf-8",
            )

            run_command(
                [
                    "node",
                    "scripts/release/daemon/_generate-manifest.js",
                    "--version",
                    "1.2.3",
                    "--dist",
                    str(temp_dir),
                    "--output",
                    str(temp_dir / "manifest.json"),
                    "--download-base-urls",
                    str(urls_file),
                    "--download-base-url",
                    "https://example.com/daemon",
                    "--allow-partial",
                ]
            )

            manifest = json.loads((temp_dir / "manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(
                manifest["download_base_urls"],
                ["https://example.com/daemon", "https://cdn.example.com/daemon"],
            )

    def test_stage_downloads_can_publish_partial_layout(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            temp_dir = pathlib.Path(temp)
            source_dir = temp_dir / "source"
            source_dir.mkdir()
            create_fake_daemon_package(source_dir, "linux", "amd64")

            output_dir = temp_dir / "daemon"
            run_command(
                [
                    "scripts/release/daemon/_stage-downloads.sh",
                    "--version",
                    "1.2.3",
                    "--source-dir",
                    str(source_dir),
                    "--output-dir",
                    str(output_dir),
                    "--base-url",
                    "https://example.com",
                    "--download-base-url",
                    "https://example.com/daemon",
                    "--allow-partial",
                ]
            )

            manifest = json.loads(
                (output_dir / "releases" / "manifest.json").read_text(encoding="utf-8")
            )
            self.assertEqual(len(manifest["packages"]), 1)
            self.assertEqual(manifest["packages"][0]["path"], "releases/1.2.3/awiki-deamon-linux-amd64.tar.gz")

    def test_installer_downloads_verifies_extracts_and_execs_token_only_install(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            temp_dir = pathlib.Path(temp)
            source_dir = temp_dir / "source"
            create_all_fake_packages(source_dir)

            output_dir = temp_dir / "daemon"
            with serve_directory(output_dir) as base_url:
                run_command(
                    [
                        "scripts/release/daemon/_stage-downloads.sh",
                        "--version",
                        "1.2.3",
                        "--source-dir",
                        str(source_dir),
                        "--output-dir",
                        str(output_dir),
                        "--base-url",
                        "https://example.com",
                        "--download-base-url",
                        base_url,
                    ]
                )

                home = temp_dir / "home"
                home.mkdir()
                run_command(
                    ["sh", str(output_dir / "install.sh"), "--token", "test-install-token"],
                    env={"HOME": str(home)},
                )

            args_path = home / "fake-awiki-deamon-args.txt"
            self.assertEqual(
                args_path.read_text(encoding="utf-8").splitlines(),
                [
                    "install",
                    "--token",
                    "test-install-token",
                    "--base-url",
                    "https://example.com",
                    "--download-base-url",
                    base_url,
                ],
            )
            bin_root = home / ".awiki-daemon" / "deamon" / "bin"
            version_dir = bin_root / "1.2.3"
            self.assertTrue((version_dir / "awiki-deamon").is_file())
            self.assertTrue((version_dir / "README.txt").is_file())
            self.assertTrue((version_dir / "LICENSE").is_file())
            self.assertTrue((version_dir / "LICENSE-APACHE").is_file())
            self.assertTrue((version_dir / "COMMERCIAL-LICENSING.md").is_file())
            self.assertTrue((version_dir / "SOURCE.md").is_file())
            self.assertEqual(
                pathlib.Path(readlink(bin_root / "current" / "awiki-deamon")),
                pathlib.Path("../1.2.3/awiki-deamon"),
            )
            self.assertEqual(
                pathlib.Path(readlink(bin_root / "current" / "awiki-deamon-runtime")),
                pathlib.Path("../1.2.3/awiki-deamon-runtime"),
            )

    def test_cleanup_script_removes_fixed_product_root_and_service_registration(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            temp_dir = pathlib.Path(temp)
            source_dir = temp_dir / "source"
            create_all_fake_packages(source_dir)
            output_dir = temp_dir / "daemon"
            run_command(
                [
                    "scripts/release/daemon/_stage-downloads.sh",
                    "--version",
                    "1.2.3",
                    "--source-dir",
                    str(source_dir),
                    "--output-dir",
                    str(output_dir),
                    "--base-url",
                    "https://example.com",
                ]
            )

            home = temp_dir / "home"
            product_root = home / ".awiki-daemon" / "deamon"
            (product_root / "state").mkdir(parents=True)
            (product_root / "state" / "daemon.db").write_text("state", encoding="utf-8")
            (product_root / "bin" / "current").mkdir(parents=True)
            (product_root / "env").mkdir(parents=True)
            launch_agent = home / "Library" / "LaunchAgents" / "ai.awiki.deamon.plist"
            launch_agent.parent.mkdir(parents=True)
            launch_agent.write_text("plist", encoding="utf-8")
            systemd_unit = home / ".config" / "systemd" / "user" / "awiki-deamon.service"
            systemd_unit.parent.mkdir(parents=True)
            systemd_unit.write_text("unit", encoding="utf-8")

            result = subprocess.run(
                ["sh", str(output_dir / "cleanup.sh"), "--yes"],
                cwd=ROOT,
                env={**os.environ, "HOME": str(home)},
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse(product_root.exists())
            self.assertFalse(launch_agent.exists())
            self.assertFalse(systemd_unit.exists())
            self.assertIn("AWiki daemon host cleanup complete", result.stderr)

    def test_installer_uses_successful_mirror_when_primary_package_is_missing(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            temp_dir = pathlib.Path(temp)
            source_dir = temp_dir / "source"
            create_all_fake_packages(source_dir)

            primary_dir = temp_dir / "primary" / "daemon"
            mirror_dir = temp_dir / "mirror" / "daemon"
            run_command(
                [
                    "scripts/release/daemon/_stage-downloads.sh",
                    "--version",
                    "1.2.3",
                    "--source-dir",
                    str(source_dir),
                    "--output-dir",
                    str(mirror_dir),
                    "--base-url",
                    "https://example.com",
                    "--download-base-url",
                    file_url(primary_dir),
                    "--download-mirror-url",
                    file_url(mirror_dir),
                ]
            )
            shutil.copytree(mirror_dir, primary_dir)
            os_name, arch = current_installer_target()
            (primary_dir / "releases" / "1.2.3" / f"awiki-deamon-{os_name}-{arch}.tar.gz").unlink()

            home = temp_dir / "home"
            home.mkdir()
            run_command(
                ["sh", str(mirror_dir / "install.sh"), "--token", "test-install-token"],
                env={"HOME": str(home)},
            )

            self.assertEqual(
                (home / "fake-awiki-deamon-args.txt").read_text(encoding="utf-8").splitlines(),
                [
                    "install",
                    "--token",
                    "test-install-token",
                    "--base-url",
                    "https://example.com",
                    "--download-base-url",
                    file_url(mirror_dir),
                ],
            )

    def test_installer_can_forward_explicit_service_base_url_from_environment(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            temp_dir = pathlib.Path(temp)
            source_dir = temp_dir / "source"
            create_all_fake_packages(source_dir)

            output_dir = temp_dir / "daemon"
            with serve_directory(output_dir) as base_url:
                run_command(
                    [
                        "scripts/release/daemon/_stage-downloads.sh",
                        "--version",
                        "1.2.3",
                        "--source-dir",
                        str(source_dir),
                        "--output-dir",
                        str(output_dir),
                        "--base-url",
                        "https://example.com",
                        "--download-base-url",
                        base_url,
                    ]
                )

                home = temp_dir / "home"
                home.mkdir()
                run_command(
                    ["sh", str(output_dir / "install.sh"), "--token", "test-install-token"],
                    env={
                        "HOME": str(home),
                        "AWIKI_DAEMON_SERVICE_BASE_URL": "http://127.0.0.1:9999",
                    },
                )

            self.assertEqual(
                (home / "fake-awiki-deamon-args.txt").read_text(encoding="utf-8").splitlines(),
                [
                    "install",
                    "--token",
                    "test-install-token",
                    "--base-url",
                    "http://127.0.0.1:9999",
                    "--download-base-url",
                    base_url,
                ],
            )

    def test_installer_template_can_infer_service_base_url_from_standard_download_base_url(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            temp_dir = pathlib.Path(temp)
            source_dir = temp_dir / "source"
            create_all_fake_packages(source_dir)

            download_root = temp_dir / "download-root"
            output_dir = download_root / "daemon"
            with serve_directory(download_root) as base_url:
                download_base_url = f"{base_url}/daemon"
                run_command(
                    [
                        "scripts/release/daemon/_stage-downloads.sh",
                        "--version",
                        "1.2.3",
                        "--source-dir",
                        str(source_dir),
                        "--output-dir",
                        str(output_dir),
                        "--download-base-url",
                        download_base_url,
                    ]
                )

                home = temp_dir / "home"
                home.mkdir()
                run_command(
                    ["sh", "scripts/release/daemon/_install.sh.template", "--token", "test-install-token"],
                    env={
                        "HOME": str(home),
                        "AWIKI_DAEMON_DOWNLOAD_BASE_URL": download_base_url,
                    },
                )

            self.assertEqual(
                (home / "fake-awiki-deamon-args.txt").read_text(encoding="utf-8").splitlines(),
                [
                    "install",
                    "--token",
                    "test-install-token",
                    "--base-url",
                    base_url,
                    "--download-base-url",
                    download_base_url,
                ],
            )

    def test_installer_rejects_archive_with_unexpected_entries(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            temp_dir = pathlib.Path(temp)
            bad_source = temp_dir / "bad-source"
            bad_source.mkdir()
            bad_os, bad_arch = current_installer_target()

            stage = temp_dir / "bad-stage"
            stage.mkdir()
            (stage / "awiki-deamon").write_text("#!/bin/sh\n", encoding="utf-8")
            (stage / "awiki-deamon").chmod(0o755)
            (stage / "awiki-deamon-runtime").symlink_to("awiki-deamon")
            (stage / "README.txt").write_text("readme\n", encoding="utf-8")
            (stage / "LICENSE").write_text("license\n", encoding="utf-8")
            (stage / "LICENSE-APACHE").write_text("Apache license\n", encoding="utf-8")
            (stage / "COMMERCIAL-LICENSING.md").write_text(
                "commercial policy\n", encoding="utf-8"
            )
            (stage / "SOURCE.md").write_text("Commit: bad-commit\n", encoding="utf-8")
            (stage / "checksums.txt").write_text("checksums\n", encoding="utf-8")
            (stage / "unexpected.txt").write_text("unexpected\n", encoding="utf-8")
            run_command(
                [
                    "tar",
                    "-C",
                    str(stage),
                    "-czf",
                    str(bad_source / f"awiki-deamon-{bad_os}-{bad_arch}.tar.gz"),
                    "awiki-deamon",
                    "awiki-deamon-runtime",
                    "README.txt",
                    "LICENSE",
                    "LICENSE-APACHE",
                    "COMMERCIAL-LICENSING.md",
                    "SOURCE.md",
                    "checksums.txt",
                    "unexpected.txt",
                ]
            )
            for os_name, arch in TARGETS:
                if (os_name, arch) != (bad_os, bad_arch):
                    create_fake_daemon_package(bad_source, os_name, arch)

            output_dir = temp_dir / "daemon"
            with serve_directory(output_dir) as base_url:
                run_command(
                    [
                        "scripts/release/daemon/_stage-downloads.sh",
                        "--version",
                        "1.2.3",
                        "--source-dir",
                        str(bad_source),
                        "--output-dir",
                        str(output_dir),
                        "--base-url",
                        "https://example.com",
                        "--download-base-url",
                        base_url,
                    ]
                )
                home = temp_dir / "home"
                home.mkdir()
                result = subprocess.run(
                    ["sh", str(output_dir / "install.sh"), "--token", "test-install-token"],
                    cwd=ROOT,
                    env={**os.environ, "HOME": str(home)},
                    text=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    check=False,
                )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("unexpected daemon package entry: unexpected.txt", result.stderr)
            self.assertFalse((home / "fake-awiki-deamon-args.txt").exists())

    def test_mirror_sync_accepts_config_only_and_pulls_from_source(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            temp_dir = pathlib.Path(temp)
            source_packages = temp_dir / "packages"
            create_all_fake_packages(source_packages)
            source_daemon = temp_dir / "source" / "daemon"
            run_command(
                [
                    "scripts/release/daemon/_stage-downloads.sh",
                    "--version",
                    "1.2.3",
                    "--source-dir",
                    str(source_packages),
                    "--output-dir",
                    str(source_daemon),
                    "--base-url",
                    "https://example.com",
                    "--download-base-url",
                    "https://source.example.com/daemon",
                ]
            )

            script_dir = temp_dir / "script"
            script_dir.mkdir()
            shutil.copy2(ROOT / "scripts/release/daemon/sync-download-mirror.sh", script_dir)
            target_dir = temp_dir / "target" / "daemon"

            with serve_directory(temp_dir / "source") as source_base:
                (script_dir / "sync-download-mirror.toml").write_text(
                    f'source_base_url = "{source_base}/daemon"\n'
                    f'target_dir = "{target_dir}"\n'
                    'keep_versions = "2"\n',
                    encoding="utf-8",
                )
                run_command([str(script_dir / "sync-download-mirror.sh")])

            self.assertTrue((target_dir / "install.sh").is_file())
            self.assertTrue((target_dir / "cleanup.sh").is_file())
            self.assertEqual(
                (target_dir / "cleanup.sh").read_text(encoding="utf-8"),
                (source_daemon / "cleanup.sh").read_text(encoding="utf-8"),
            )
            self.assertEqual(
                json.loads((target_dir / "releases" / "manifest.json").read_text(encoding="utf-8")),
                json.loads((source_daemon / "releases" / "manifest.json").read_text(encoding="utf-8")),
            )
            manifest = json.loads((target_dir / "releases" / "manifest.json").read_text(encoding="utf-8"))
            for package in manifest["packages"]:
                self.assertTrue((target_dir / package["path"]).is_file())
                self.assertEqual(
                    hashlib.sha256((target_dir / package["path"]).read_bytes()).hexdigest(),
                    package["sha256"],
                )
            self.assertTrue((target_dir / "releases" / "1.2.3" / "checksums.txt").is_file())

            result = subprocess.run(
                [str(script_dir / "sync-download-mirror.sh"), "--source-base-url", "http://example.com"],
                cwd=ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("accepts no arguments", result.stderr)


if __name__ == "__main__":
    unittest.main()
