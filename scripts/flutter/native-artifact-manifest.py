#!/usr/bin/env python3
"""Write and verify provenance for Apple awiki_im_core XCFrameworks."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import pathlib
import plistlib
import subprocess
import sys
import tempfile
from typing import Any


SCHEMA = "awiki.im-core.native-artifact.v1"
ROOT = pathlib.Path(__file__).resolve().parents[2]
ANP_ROOT = (ROOT.parent / "anp" / "anp").resolve()
SOURCE_INPUTS = (
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "crates/im-core",
    "crates/im-core-dart",
)
ANP_INPUTS = ("rust",)
PLATFORM_CONFIG = {
    "ios": ROOT / "packages/awiki_im_core/ios/Frameworks",
    "macos": ROOT / "packages/awiki_im_core/macos/Frameworks",
}


class ManifestError(RuntimeError):
    pass


def run(*args: str, cwd: pathlib.Path = ROOT) -> bytes:
    try:
        return subprocess.check_output(args, cwd=cwd, stderr=subprocess.STDOUT)
    except subprocess.CalledProcessError as error:
        output = error.output.decode("utf-8", errors="replace").strip()
        raise ManifestError(f"command failed: {' '.join(args)}\n{output}") from error


def sha256_bytes(content: bytes) -> str:
    return hashlib.sha256(content).hexdigest()


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def git_input_files(repo: pathlib.Path, pathspecs: tuple[str, ...]) -> list[str]:
    output = run(
        "git",
        "ls-files",
        "-co",
        "--exclude-standard",
        "-z",
        "--",
        *pathspecs,
        cwd=repo,
    )
    return sorted({item.decode("utf-8") for item in output.split(b"\0") if item})


def tree_digest(repo: pathlib.Path, pathspecs: tuple[str, ...]) -> str:
    digest = hashlib.sha256()
    files = git_input_files(repo, pathspecs)
    if not files:
        raise ManifestError(f"no source inputs found under {repo}")
    for relative in files:
        path = repo / relative
        if not path.is_file():
            continue
        encoded = relative.encode("utf-8")
        digest.update(len(encoded).to_bytes(8, "big"))
        digest.update(encoded)
        content = path.read_bytes()
        digest.update(len(content).to_bytes(8, "big"))
        digest.update(content)
    return digest.hexdigest()


def repository_record(
    name: str,
    repo: pathlib.Path,
    pathspecs: tuple[str, ...],
) -> dict[str, Any]:
    if not (repo / ".git").exists():
        raise ManifestError(f"required Git checkout is missing: {repo}")
    revision = run("git", "rev-parse", "HEAD", cwd=repo).decode().strip()
    return {
        "name": name,
        "revision": revision,
        "inputSha256": tree_digest(repo, pathspecs),
    }


def generated_dart_digest() -> str:
    generated = ROOT / "packages/awiki_im_core/lib/src/generated"
    digest = hashlib.sha256()
    files = sorted(path for path in generated.rglob("*.dart") if path.is_file())
    if not files:
        raise ManifestError("generated Dart bridge files are missing")
    for path in files:
        relative = path.relative_to(generated).as_posix().encode("utf-8")
        digest.update(len(relative).to_bytes(8, "big"))
        digest.update(relative)
        content = path.read_bytes()
        digest.update(len(content).to_bytes(8, "big"))
        digest.update(content)
    return digest.hexdigest()


def bridge_record() -> dict[str, str]:
    rust_generated = ROOT / "crates/im-core-dart/src/frb_generated.rs"
    if not rust_generated.is_file():
        raise ManifestError("generated Rust bridge file is missing")
    return {
        "rustGeneratedSha256": sha256_file(rust_generated),
        "dartGeneratedTreeSha256": generated_dart_digest(),
    }


def xcframework_record(platform: str) -> dict[str, Any]:
    frameworks = PLATFORM_CONFIG[platform]
    xcframework = frameworks / "AwikiImCore.xcframework"
    info_path = xcframework / "Info.plist"
    if not info_path.is_file():
        raise ManifestError(f"XCFramework Info.plist is missing: {info_path}")
    with info_path.open("rb") as source:
        info = plistlib.load(source)
    available = info.get("AvailableLibraries")
    if not isinstance(available, list) or not available:
        raise ManifestError("XCFramework has no AvailableLibraries entries")

    binaries: list[dict[str, Any]] = []
    for library in available:
        if not isinstance(library, dict) or library.get("SupportedPlatform") != platform:
            continue
        identifier = library.get("LibraryIdentifier")
        library_path = library.get("LibraryPath")
        declared_architectures = library.get("SupportedArchitectures")
        if (
            not isinstance(identifier, str)
            or not isinstance(library_path, str)
            or not isinstance(declared_architectures, list)
            or not all(isinstance(item, str) for item in declared_architectures)
        ):
            raise ManifestError("XCFramework library metadata is incomplete")
        binary = xcframework / identifier / library_path
        if not binary.is_file():
            raise ManifestError(f"XCFramework binary is missing: {binary}")
        actual_architectures = sorted(
            run("lipo", "-archs", str(binary)).decode().strip().split()
        )
        if actual_architectures != sorted(declared_architectures):
            raise ManifestError(
                f"architecture metadata mismatch for {binary}: "
                f"plist={sorted(declared_architectures)}, binary={actual_architectures}"
            )
        binaries.append(
            {
                "path": binary.relative_to(frameworks).as_posix(),
                "architectures": actual_architectures,
                "sizeBytes": binary.stat().st_size,
                "sha256": sha256_file(binary),
            }
        )
    if not binaries:
        raise ManifestError(f"XCFramework has no {platform} libraries")
    return {
        "infoPlistSha256": sha256_file(info_path),
        "binaries": sorted(binaries, key=lambda entry: entry["path"]),
    }


def current_source_record() -> dict[str, Any]:
    return {
        "repositories": [
            repository_record("awiki-cli-rs2", ROOT, SOURCE_INPUTS),
            repository_record("anp", ANP_ROOT, ANP_INPUTS),
        ],
        "bridge": bridge_record(),
    }


def manifest_path(platform: str) -> pathlib.Path:
    return PLATFORM_CONFIG[platform] / "AwikiImCore.artifact.json"


def write_manifest(platform: str, targets: str, features: str) -> None:
    payload = {
        "schema": SCHEMA,
        "platform": platform,
        "builtAt": dt.datetime.now(dt.timezone.utc)
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z"),
        "source": current_source_record(),
        "build": {
            "targets": sorted(item for item in targets.split(",") if item),
            "features": sorted(item for item in features.split(",") if item),
            "rustc": run("rustc", "--version").decode().strip(),
        },
        "artifact": xcframework_record(platform),
    }
    destination = manifest_path(platform)
    destination.parent.mkdir(parents=True, exist_ok=True)
    content = json.dumps(payload, indent=2, sort_keys=True) + "\n"
    with tempfile.NamedTemporaryFile(
        "w",
        encoding="utf-8",
        dir=destination.parent,
        delete=False,
    ) as temporary:
        temporary.write(content)
        temporary.flush()
        os.fsync(temporary.fileno())
        temporary_path = pathlib.Path(temporary.name)
    os.replace(temporary_path, destination)
    print(f"wrote native artifact manifest: {destination}")


def verify_manifest(platform: str) -> None:
    source_path = manifest_path(platform)
    if not source_path.is_file():
        raise ManifestError(
            f"native artifact manifest is missing: {source_path}\n"
            f"rebuild with scripts/flutter/build-sdk-native.sh --{platform}-only"
        )
    try:
        recorded = json.loads(source_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ManifestError(f"native artifact manifest is invalid: {source_path}") from error
    if recorded.get("schema") != SCHEMA or recorded.get("platform") != platform:
        raise ManifestError("native artifact manifest schema or platform is invalid")

    differences: list[str] = []
    current_source = current_source_record()
    if recorded.get("source") != current_source:
        differences.append("source revision, source inputs, or generated bridge changed")
    current_artifact = xcframework_record(platform)
    if recorded.get("artifact") != current_artifact:
        differences.append("XCFramework architecture, metadata, or binary digest changed")
    if differences:
        detail = "; ".join(differences)
        raise ManifestError(
            f"native artifact provenance mismatch: {detail}\n"
            f"rebuild with scripts/flutter/build-sdk-native.sh --{platform}-only"
        )
    print(f"native artifact verified: {platform} ({source_path})")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    writer = subparsers.add_parser("write", help="write a manifest after a build")
    writer.add_argument("--platform", choices=sorted(PLATFORM_CONFIG), required=True)
    writer.add_argument("--targets", required=True)
    writer.add_argument("--features", required=True)
    verifier = subparsers.add_parser("verify", help="verify a built artifact")
    verifier.add_argument("--platform", choices=sorted(PLATFORM_CONFIG), required=True)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        if args.command == "write":
            write_manifest(args.platform, args.targets, args.features)
        else:
            verify_manifest(args.platform)
    except ManifestError as error:
        print(f"native artifact verification failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
