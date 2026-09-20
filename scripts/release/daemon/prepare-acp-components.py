#!/usr/bin/env python3
"""Build pinned ACP components without installing or changing any host Agent CLI.

Build-time network access is limited to locked npm artifacts. Host Node is
resolved and validated by the daemon. Runtime installation never invokes this builder.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import tempfile
import urllib.parse

SOURCE = Path(__file__).resolve().parent / "acp"
ROOT = SOURCE.parents[3]


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def validate_lock(lock):
    if lock.get("lockfileVersion") != 3:
        raise ValueError("ACP components require an npm v3 lock")
    for name, package in lock["packages"].items():
        if not name:
            continue
        source = urllib.parse.urlsplit(package.get("resolved", ""))
        if (package.get("link") or source.scheme != "https"
                or source.hostname != "registry.npmjs.org" or source.username
                or source.password or not package.get("integrity", "").startswith("sha512-")):
            raise ValueError(f"ACP component is not a fixed registry artifact: {name}")


def prune_development_assets(root):
    """Remove non-runtime assets only; retain all JS, JSON, source TS and legal notices."""
    import re
    removed = 0
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            raise ValueError("Unexpected component symlink")
        if not path.is_file():
            continue
        parts = path.relative_to(root).parts
        if any(part.lower().startswith(("license", "notice", "copying", "copyright")) for part in parts):
            continue
        name = path.name
        auxiliary = (name.endswith((".map", ".d.ts", ".d.mts", ".d.cts", ".md", ".mdx"))
            or any(part in {"test", "tests", "__tests__", "example", "examples", "benchmark", "benchmarks"} for part in parts)
            or re.search(r"\.(test|spec)\.(js|ts|cjs|mjs)$", name))
        if auxiliary:
            removed += path.stat().st_size
            path.unlink()
    # Empty directories are not payload and need not enter the archive.
    for path in sorted(root.rglob("*"), key=lambda p: len(p.parts), reverse=True):
        if path.is_dir() and not any(path.iterdir()):
            path.rmdir()
    return removed


def write_legacy_upgrade_launcher(bundle):
    # 0.1.101's updater requires these filenames before it can launch a newer
    # daemon. Keep a tiny host-Node launcher, never a bundled Node binary.
    (bundle / "node").write_text('#!/bin/sh\n# Compatibility launcher; Node.js is supplied by the host.\nexec /usr/bin/env node "$@"\n')
    (bundle / "node").chmod(0o755)
    (bundle / "LICENSE.node").write_text(
        "No Node.js binary is distributed in this package.\n"
        "The compatibility launcher is covered by the package root licenses.\n"
        "These filenames preserve upgrades from Daemon 0.1.101.\n")


def component_files(root):
    files = {}
    for path in sorted(root.rglob("*")):
        relative = path.relative_to(root).as_posix()
        if path.is_symlink():
            raise ValueError(f"Component symlinks are not supported: {relative}")
        if any(c in relative for c in "\r\n\\\0") or any(part in {".", ".."} for part in PurePosixPath(relative).parts):
            raise ValueError("Unsafe component filename")
        if path.is_file():
            files[relative] = sha256(path)
        elif not path.is_dir():
            raise ValueError(f"Component must be a regular file or directory: {relative}")
    return files


def verify_adapters(root, specification):
    for adapter in specification["adapters"].values():
        package_dir = root / "node_modules" / adapter["package"]
        package = json.loads((package_dir / "package.json").read_text())
        if package.get("version") != adapter["version"]:
            raise ValueError("Installed adapter differs from the pinned component version")
        if not (root / adapter["entry"]).is_file():
            raise ValueError("ACP adapter entrypoint is missing")
    # Native vendor CLIs are selected by the official executable overrides. They
    # are not redistributed, and optional platform packages must remain absent.
    for namespace, prefix in [("@openai", "codex-"), ("@anthropic-ai", "claude-agent-sdk-")]:
        if any((root / "node_modules" / namespace).glob(prefix + "*")):
            raise ValueError("Unexpected bundled native Agent CLI")


def prepare(platform, output, cache):
    specification = json.loads((SOURCE / "components.json").read_text())
    lock_path = SOURCE / "package-lock.json"
    validate_lock(json.loads(lock_path.read_text()))
    if output.exists():
        raise ValueError("Component destination already exists; use a fresh staging directory")
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="acp-stage-", dir=output.parent) as temporary:
        stage = Path(temporary)
        bundle = stage / "bundle"
        bundle.mkdir()
        manifest = {
            "schema_version": 1,
            "platform": platform,
            "available": platform in specification["platforms"],
            "runtime": specification["runtime"],
            "package_lock_sha256": sha256(lock_path),
            "adapters": specification["adapters"],
        }
        if manifest["available"]:
            install = stage / "npm"
            install.mkdir()
            for name in ("package.json", "package-lock.json"):
                shutil.copy2(SOURCE / name, install / name)
                shutil.copy2(SOURCE / name, bundle / name)
            subprocess.run(["npm", "ci", "--omit=dev", "--omit=optional", "--ignore-scripts",
                            "--no-audit", "--no-fund"], cwd=install, check=True)
            shutil.copytree(install / "node_modules", bundle / "node_modules", symlinks=True,
                            ignore=lambda _directory, names: {".bin"} & set(names))
            verify_adapters(bundle, specification)
            prune_development_assets(bundle / "node_modules")
            write_legacy_upgrade_launcher(bundle)
        else:
            manifest["unavailable_reason"] = "adapter_platform_unsupported"
        manifest["files"] = component_files(bundle)
        (bundle / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
        bundle.rename(output)
    print(json.dumps({"platform": platform, "available": manifest["available"],
                      "files": len(manifest["files"]), "bytes": sum(p.stat().st_size for p in output.rglob("*") if p.is_file())}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--os", required=True, choices=["darwin", "linux"])
    parser.add_argument("--arch", required=True, choices=["arm64", "amd64"])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--cache", type=Path, default=ROOT / "target" / "acp-component-downloads")
    args = parser.parse_args()
    prepare(f"{args.os}-{args.arch}", args.output.resolve(), args.cache.resolve())


if __name__ == "__main__":
    main()
