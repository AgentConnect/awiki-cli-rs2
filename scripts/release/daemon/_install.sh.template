#!/bin/sh
set -eu

BASE_URL="${AWIKI_DAEMON_BASE_URL:-${AWIKI_DAEMON_SERVICE_BASE_URL:-__AWIKI_DAEMON_BASE_URL__}}"
DOWNLOAD_BASE_URL="${AWIKI_DAEMON_DOWNLOAD_BASE_URL:-__AWIKI_DAEMON_DOWNLOAD_BASE_URL__}"
TOKEN=""
VERSION=""
STATE_ROOT=""
FOREGROUND=0

usage() {
  cat <<'USAGE'
Usage:
  install.sh --token <token> [--version <version>] [--state-root <path>] [--foreground]
USAGE
}

die() {
  echo "Error: $*" >&2
  exit 1
}

normalize_config_value() {
  base_placeholder="__AWIKI_DAEMON_""BASE_URL__"
  download_placeholder="__AWIKI_DAEMON_""DOWNLOAD_BASE_URL__"
  case "$1" in
    "$base_placeholder"|"$download_placeholder")
      printf '\n'
      ;;
    *)
      printf '%s\n' "$1"
      ;;
  esac
}

trim_trailing_slash() {
  value="$1"
  while [ "${value%/}" != "$value" ]; do
    value="${value%/}"
  done
  printf '%s\n' "$value"
}

infer_base_url_from_download_base_url() {
  download_base="$(trim_trailing_slash "$1")"
  case "$download_base" in
    http://*/daemon|https://*/daemon)
      printf '%s\n' "${download_base%/daemon}"
      ;;
    *)
      return 1
      ;;
  esac
}

validate_version_segment() {
  [ -n "$1" ] || die "daemon package version is empty"
  case "$1" in
    .*|*..*|*[!0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz._-]*)
      die "daemon package version contains unsupported characters"
      ;;
  esac
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --token)
      [ "$#" -ge 2 ] || die "--token requires a value"
      TOKEN="$2"
      shift 2
      ;;
    --version)
      [ "$#" -ge 2 ] || die "--version requires a value"
      VERSION="$2"
      shift 2
      ;;
    --state-root)
      [ "$#" -ge 2 ] || die "--state-root requires a path"
      STATE_ROOT="$2"
      shift 2
      ;;
    --foreground)
      FOREGROUND=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

BASE_URL="$(normalize_config_value "$BASE_URL")"
DOWNLOAD_BASE_URL="$(normalize_config_value "$DOWNLOAD_BASE_URL")"
if [ -n "$DOWNLOAD_BASE_URL" ]; then
  DOWNLOAD_BASE_URL="$(trim_trailing_slash "$DOWNLOAD_BASE_URL")"
fi
if [ -z "$BASE_URL" ] && [ -n "$DOWNLOAD_BASE_URL" ]; then
  if inferred_base_url="$(infer_base_url_from_download_base_url "$DOWNLOAD_BASE_URL")"; then
    BASE_URL="$inferred_base_url"
  fi
fi
if [ -n "$BASE_URL" ]; then
  BASE_URL="$(trim_trailing_slash "$BASE_URL")"
fi

[ -n "$TOKEN" ] || die "--token is required"
[ -n "$BASE_URL" ] || die "daemon base URL is not configured"
[ -n "$DOWNLOAD_BASE_URL" ] || die "daemon download base URL is not configured"
case "$BASE_URL" in
  http://*|https://*) ;;
  *) die "daemon base URL must start with http:// or https://" ;;
esac
case "$DOWNLOAD_BASE_URL" in
  http://*|https://*|file://*|/*|./*|../*) ;;
  *) die "daemon download base URL must be a URL or local path" ;;
esac

command -v curl >/dev/null 2>&1 || die "curl is required"
command -v tar >/dev/null 2>&1 || die "tar is required"

download_to() {
  src="$1"
  dest="$2"
  case "$src" in
    file://*)
      "$PYTHON_BIN" - "$src" "$dest" <<'PY' || die "copy local file URL failed"
import shutil
import sys
from urllib.parse import unquote, urlparse

src, dest = sys.argv[1:3]
path = unquote(urlparse(src).path)
shutil.copyfile(path, dest)
PY
      ;;
    /*|./*|../*)
      cp "$src" "$dest"
      ;;
    *)
      curl -fsSL "$src" -o "$dest"
      ;;
  esac
}

case "$(uname -s)" in
  Darwin) OS="darwin" ;;
  Linux) OS="linux" ;;
  *) die "当前系统暂不支持 awiki daemon。第一版支持 macOS 和 Linux。" ;;
esac

case "$(uname -m)" in
  x86_64|amd64) ARCH="amd64" ;;
  arm64|aarch64) ARCH="arm64" ;;
  *) die "当前系统暂不支持 awiki daemon。第一版支持 macOS 和 Linux。" ;;
esac

TMP_DIR="${TMPDIR:-/tmp}/awiki-daemon-install.$$"
mkdir -p "$TMP_DIR"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

validate_archive_entries() {
  "$PYTHON_BIN" - "$ARCHIVE_PATH" <<'PY' || die "daemon package validation failed"
import sys
import tarfile

archive_path = sys.argv[1]
expected = {
    "awiki-deamon": "file",
    "awiki-deamon-runtime": "runtime",
    "README.txt": "file",
    "LICENSE": "file",
    "checksums.txt": "file",
}
seen = set()

try:
    with tarfile.open(archive_path, "r:gz") as archive:
        for member in archive.getmembers():
            name = member.name
            while name.startswith("./"):
                name = name[2:]
            if (
                not name
                or name.startswith("/")
                or name in {".", ".."}
                or name.startswith("../")
                or "/../" in name
                or name.endswith("/..")
                or "//" in name
            ):
                raise SystemExit(f"unsafe daemon package path: {name or member.name}")
            if name not in expected:
                raise SystemExit(f"unexpected daemon package entry: {name}")
            if name in seen:
                raise SystemExit(f"duplicate daemon package entry: {name}")
            seen.add(name)

            expected_kind = expected[name]
            if expected_kind == "file":
                if not member.isfile():
                    raise SystemExit(f"daemon package entry must be a regular file: {name}")
            elif member.issym():
                if member.linkname != "awiki-deamon":
                    raise SystemExit("awiki-deamon-runtime symlink target is unsupported")
            elif not member.isfile():
                raise SystemExit("awiki-deamon-runtime must be a regular file or symlink")

        missing = sorted(set(expected) - seen)
        if missing:
            raise SystemExit(f"daemon package missing {', '.join(missing)}")
except tarfile.TarError as exc:
    raise SystemExit(f"daemon package listing failed: {exc}") from exc
PY
}

validate_extracted_package() {
  [ -f "$INSTALL_DIR/awiki-deamon" ] || die "daemon package does not contain awiki-deamon"
  [ ! -L "$INSTALL_DIR/awiki-deamon" ] || die "awiki-deamon must not be a symlink"
  [ -e "$INSTALL_DIR/awiki-deamon-runtime" ] || die "daemon package does not contain awiki-deamon-runtime"
  if [ -L "$INSTALL_DIR/awiki-deamon-runtime" ]; then
    command -v readlink >/dev/null 2>&1 || die "readlink is required to validate daemon runtime symlink"
    runtime_target="$(readlink "$INSTALL_DIR/awiki-deamon-runtime")"
    [ "$runtime_target" = "awiki-deamon" ] || die "awiki-deamon-runtime symlink target is unsupported"
  else
    [ -f "$INSTALL_DIR/awiki-deamon-runtime" ] || die "awiki-deamon-runtime must be a file or symlink"
  fi
  [ -f "$INSTALL_DIR/README.txt" ] || die "daemon package does not contain README.txt"
  [ -f "$INSTALL_DIR/LICENSE" ] || die "daemon package does not contain LICENSE"
  [ -f "$INSTALL_DIR/checksums.txt" ] || die "daemon package does not contain checksums.txt"
}

PYTHON_BIN=""
if command -v python3 >/dev/null 2>&1; then
  PYTHON_BIN="python3"
elif command -v python >/dev/null 2>&1; then
  PYTHON_BIN="python"
else
  die "python3 is required to parse manifest.json"
fi

MANIFEST_URL="${DOWNLOAD_BASE_URL%/}/releases/manifest.json"
MANIFEST_PATH="${TMP_DIR}/manifest.json"
download_to "$MANIFEST_URL" "$MANIFEST_PATH"

selection="$($PYTHON_BIN - "$MANIFEST_PATH" "$OS" "$ARCH" "$VERSION" <<'PY'
import json
import sys

manifest_path, os_name, arch_name, requested = sys.argv[1:5]
manifest = json.load(open(manifest_path, "r", encoding="utf-8"))
version = requested or manifest.get("latest") or ""
for package in manifest.get("packages", []):
    if (
        str(package.get("version", "")) == version
        and package.get("os") == os_name
        and package.get("arch") == arch_name
    ):
        print(package.get("version", ""))
        print(package.get("url", ""))
        print(package.get("sha256", ""))
        break
else:
    raise SystemExit(f"no daemon package for {os_name}-{arch_name} version {version or '<latest>'}")
PY
)"

PKG_VERSION="$(printf '%s\n' "$selection" | sed -n '1p')"
PKG_URL="$(printf '%s\n' "$selection" | sed -n '2p')"
PKG_SHA="$(printf '%s\n' "$selection" | sed -n '3p')"

[ -n "$PKG_VERSION" ] || die "manifest package version is empty"
[ -n "$PKG_URL" ] || die "manifest package URL is empty"
[ -n "$PKG_SHA" ] || die "manifest package sha256 is empty"
validate_version_segment "$PKG_VERSION"

ARCHIVE_PATH="${TMP_DIR}/awiki-deamon-${OS}-${ARCH}.tar.gz"
download_to "$PKG_URL" "$ARCHIVE_PATH"

if command -v shasum >/dev/null 2>&1; then
  ACTUAL_SHA="$(shasum -a 256 "$ARCHIVE_PATH" | awk '{print $1}')"
elif command -v sha256sum >/dev/null 2>&1; then
  ACTUAL_SHA="$(sha256sum "$ARCHIVE_PATH" | awk '{print $1}')"
else
  die "sha256 verification requires shasum or sha256sum"
fi
[ "$ACTUAL_SHA" = "$PKG_SHA" ] || die "daemon package sha256 mismatch"

validate_archive_entries

INSTALL_DIR="${HOME}/.awiki-daemon/deamon/bin/${PKG_VERSION}"
CURRENT_DIR="${HOME}/.awiki-daemon/deamon/bin/current"
EXTRACT_DIR="${TMP_DIR}/extract/${PKG_VERSION}"
mkdir -p "$EXTRACT_DIR" "$CURRENT_DIR"
tar -C "$EXTRACT_DIR" -xzf "$ARCHIVE_PATH"
INSTALL_DIR="$EXTRACT_DIR"
validate_extracted_package
chmod 0755 "$INSTALL_DIR/awiki-deamon" 2>/dev/null || true
chmod 0755 "$INSTALL_DIR/awiki-deamon-runtime" 2>/dev/null || true
FINAL_INSTALL_DIR="${HOME}/.awiki-daemon/deamon/bin/${PKG_VERSION}"
rm -rf "$FINAL_INSTALL_DIR"
mkdir -p "$(dirname "$FINAL_INSTALL_DIR")"
mv "$EXTRACT_DIR" "$FINAL_INSTALL_DIR"
INSTALL_DIR="$FINAL_INSTALL_DIR"

rm -f "$CURRENT_DIR/awiki-deamon" "$CURRENT_DIR/awiki-deamon-runtime"
ln -s "../${PKG_VERSION}/awiki-deamon" "$CURRENT_DIR/awiki-deamon"
if [ -e "$INSTALL_DIR/awiki-deamon-runtime" ]; then
  ln -s "../${PKG_VERSION}/awiki-deamon-runtime" "$CURRENT_DIR/awiki-deamon-runtime"
else
  ln -s "../${PKG_VERSION}/awiki-deamon" "$CURRENT_DIR/awiki-deamon-runtime"
fi

set -- "$CURRENT_DIR/awiki-deamon" install --token "$TOKEN"
set -- "$@" --base-url "$BASE_URL" --download-base-url "$DOWNLOAD_BASE_URL"
if [ -n "$STATE_ROOT" ]; then
  set -- "$@" --state-root "$STATE_ROOT"
fi
if [ "$FOREGROUND" = "1" ]; then
  set -- "$@" --foreground
fi
exec "$@"
