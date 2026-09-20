#!/usr/bin/env python3
"""Build pinned ACP components without installing or changing any host Agent CLI.

Build-time network access is limited to locked npm artifacts and a checksummed
official Node archive. Runtime installation never invokes this builder.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import tarfile
import tempfile
import urllib.parse
import urllib.request

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


def download_node(spec, version, cache):
    cache.mkdir(parents=True, exist_ok=True)
    destination = cache / spec["filename"]
    if destination.is_file() and sha256(destination) == spec["sha256"]:
        return destination
    url = f"https://nodejs.org/dist/v{version}/{spec['filename']}"
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=cache, prefix="node-download-", delete=False) as output:
            temporary = Path(output.name)
            with urllib.request.urlopen(url, timeout=60) as source:
                shutil.copyfileobj(source, output)
        if sha256(temporary) != spec["sha256"]:
            raise ValueError("Official Node artifact checksum mismatch")
        temporary.replace(destination)
        return destination
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def extract_node(archive, destination):
    """Extract exactly node and its notices; do not unpack an arbitrary archive."""
    expected_root = archive.name.removesuffix(".tar.gz")
    wanted = {f"{expected_root}/bin/node": "node", f"{expected_root}/LICENSE": "LICENSE.node"}
    found = set()
    with tarfile.open(archive, "r:gz") as package:
        for member in package:
            if member.name not in wanted:
                continue
            if member.name in found or not member.isfile():
                raise ValueError("Invalid or duplicate Node runtime entry")
            found.add(member.name)
            with package.extractfile(member) as source, (destination / wanted[member.name]).open("wb") as output:
                shutil.copyfileobj(source, output)
    if found != set(wanted):
        raise ValueError("Node runtime or license is missing")
    (destination / "node").chmod(0o755)


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
            "available": platform in specification["node_archives"],
            "node_version": specification["node_version"],
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
            node = specification["node_archives"][platform]
            archive = download_node(node, specification["node_version"], cache)
            extract_node(archive, bundle)
            manifest["node_archive_sha256"] = node["sha256"]
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
