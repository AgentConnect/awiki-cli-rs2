#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

SCRIPT_NAME="scripts/release/release-one-click.sh"
NPM_PACKAGE="@awiki/cli"
GITHUB_OWNER="${GITHUB_OWNER:-AgentConnect}"
GITHUB_REPO="${GITHUB_REPO:-awiki-cli}"
GITEE_OWNER="${GITEE_OWNER:-agentconnect}"
GITEE_REPO="${GITEE_REPO:-awiki-cli}"
GITEE_GIT_URL="${GITEE_GIT_URL:-git@gitee.com:agentconnect/awiki-cli.git}"
NPM_USERCONFIG_TO_CLEAN=""

cleanup() {
  if [[ -n "${NPM_USERCONFIG_TO_CLEAN}" ]]; then
    rm -f "${NPM_USERCONFIG_TO_CLEAN}"
  fi
}
trap cleanup EXIT

usage() {
  cat <<'EOF'
Usage: scripts/release/release-one-click.sh [version] [options]

Publishes an awiki-cli release end-to-end:
  1. Load local release environment.
  2. Optionally export a configured proxy for GitHub/npm access.
  3. Update package.json, Cargo.toml, and Cargo.lock release versions.
  4. Check package/Cargo version consistency and run tests.
  5. Commit and push the package metadata change.
  6. Create and push the release tag.
  7. Wait for the GitHub release workflow/assets.
  8. Mirror the GitHub release to Gitee.
  9. Publish or verify the npm package.

If version is omitted, package.json.version is used.

Options:
  --channel stable|beta|prerelease
      Release lane. Defaults to stable for plain versions and beta for
      pre-release versions.
  --dist-tag <tag>
      npm dist-tag. Defaults to latest for stable releases, or the pre-release
      identifier such as beta for pre-releases.
  --min-supported-version <version>
      Value for package.json.awikiCli.minSupportedVersion. Defaults to version.
  --env-file <path>
      Local env file to source. Defaults to scripts/release/release.env.local.
  --skip-tests
      Do not run Rust unit tests.
  --skip-gitee
      Do not mirror release assets to Gitee.
  --skip-npm
      Do not publish or verify npm.
  --skip-wait
      Do not wait for GitHub Actions/release assets.
  --no-commit
      Update package.json but stop before committing/tagging.
  --no-proxy
      Disable script-level proxy exports for this invocation.
  -h, --help
      Show this help.

Local environment:
  Copy scripts/release/release.env.example to scripts/release/release.env.local
  and fill GITHUB_TOKEN, GITEE_TOKEN, NODE_AUTH_TOKEN as needed.

Examples:
  scripts/release/release-one-click.sh 0.0.1-beta.2 --channel beta
  scripts/release/release-one-click.sh 1.0.0 --channel stable
EOF
}

die() {
  echo "Error: $*" >&2
  exit 1
}

require_command() {
  local cmd="$1"
  if ! command -v "${cmd}" >/dev/null 2>&1; then
    die "${cmd} is required to run ${SCRIPT_NAME}"
  fi
}

normalize_proxy_value() {
  local raw="${1:-}"

  case "${raw}" in
    http_proxy=*|https_proxy=*|HTTP_PROXY=*|HTTPS_PROXY=*|all_proxy=*|ALL_PROXY=*|no_proxy=*|NO_PROXY=*)
      raw="${raw#*=}"
      ;;
  esac

  raw="${raw%\"}"
  raw="${raw#\"}"
  raw="${raw%\'}"
  raw="${raw#\'}"

  printf '%s\n' "${raw}"
}

load_env_file() {
  local env_file="$1"
  if [[ -f "${env_file}" ]]; then
    echo "Loading local release environment from ${env_file}"
    set -a
    # shellcheck source=/dev/null
    source "${env_file}"
    set +a
  fi
}

read_package_version() {
  node - package.json <<'NODE'
const fs = require('fs');
const pkg = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
process.stdout.write(typeof pkg.version === 'string' ? pkg.version.trim() : '');
NODE
}

update_package_json() {
  local version="$1"
  local min_supported_version="$2"

  node - "${version}" "${min_supported_version}" <<'NODE'
const fs = require('fs');
const version = process.argv[2];
const minSupportedVersion = process.argv[3];
const pkg = JSON.parse(fs.readFileSync('package.json', 'utf8'));
pkg.version = version;
pkg.awikiCli = pkg.awikiCli && typeof pkg.awikiCli === 'object' ? pkg.awikiCli : {};
pkg.awikiCli.minSupportedVersion = minSupportedVersion;
fs.writeFileSync('package.json', `${JSON.stringify(pkg, null, 2)}\n`);
NODE
}

update_cargo_manifest_version() {
  local version="$1"

  node - "${version}" <<'NODE'
const fs = require('fs');
const version = process.argv[2];
const path = 'crates/awiki-cli/Cargo.toml';
const input = fs.readFileSync(path, 'utf8');
let inPackage = false;
let replaced = false;
const output = input.split(/\n/).map((line) => {
  const trimmed = line.trim();
  if (trimmed.startsWith('[')) {
    inPackage = trimmed === '[package]';
  }
  if (inPackage && !replaced && /^\s*version\s*=/.test(line)) {
    replaced = true;
    return line.replace(/version\s*=\s*"[^"]*"/, `version = "${version}"`);
  }
  return line;
}).join('\n');
if (!replaced) {
  throw new Error(`${path} is missing [package] version`);
}
fs.writeFileSync(path, output);
NODE
}

json_value() {
  local json_file="$1"
  local expression="$2"

  node - "${json_file}" "${expression}" <<'NODE'
const fs = require('fs');
const file = process.argv[2];
const expression = process.argv[3];
const data = JSON.parse(fs.readFileSync(file, 'utf8'));
const value = Function('data', `return (${expression});`)(data);
if (value === undefined || value === null) {
  process.exit(0);
}
process.stdout.write(typeof value === 'object' ? JSON.stringify(value) : String(value));
NODE
}

ensure_semverish() {
  local version="$1"
  if [[ ! "${version}" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
    die "version must look like semver, got ${version}"
  fi
}

ensure_clean_worktree() {
  if [[ -n "$(git status --porcelain)" ]]; then
    git status --short >&2
    die "working tree is not clean; commit or stash changes before running ${SCRIPT_NAME}"
  fi
}

configure_proxy() {
  local proxy_url="${AWIKI_RELEASE_PROXY_URL:-}"
  proxy_url="$(normalize_proxy_value "${proxy_url}")"

  if [[ -z "${proxy_url}" ]]; then
    echo "No release proxy configured; using current system/terminal network defaults."
    return
  fi

  export http_proxy="${proxy_url}"
  export https_proxy="${proxy_url}"
  export HTTP_PROXY="${proxy_url}"
  export HTTPS_PROXY="${proxy_url}"
  echo "Using proxy for GitHub/npm network access: ${proxy_url}"
}

run_tests() {
  if [[ "${RUN_TESTS}" != "1" ]]; then
    echo "Skipping tests (--skip-tests)."
    return
  fi

  require_command cargo
  echo "Running scripts/test-unit.sh"
  scripts/test-unit.sh
}

commit_package_change() {
  local version="$1"
  local min_supported_version="$2"
  local tested_summary
  local not_tested_summary

  if git diff --quiet -- package.json crates/awiki-cli/Cargo.toml Cargo.lock; then
    echo "package.json, Cargo.toml, and Cargo.lock already match version ${version}; no metadata commit needed."
  else
    git add package.json crates/awiki-cli/Cargo.toml Cargo.lock

    if [[ "${RUN_TESTS}" == "1" ]]; then
      tested_summary="scripts/test-unit.sh"
      not_tested_summary="External GitHub/Gitee/npm publication before this release commit is pushed."
    else
      tested_summary="Not run by this invocation (--skip-tests)."
      not_tested_summary="scripts/test-unit.sh; external GitHub/Gitee/npm publication before this release commit is pushed."
    fi

    git commit -F - <<EOF
Prepare awiki-cli ${version} release

Package and crate metadata are advanced before tagging so GitHub
Release, npm package metadata, Cargo metadata, and installer download
URLs all resolve the same artifact version.

Constraint: Existing release tag scripts read package.json.version as the public source of truth.
Constraint: awikiCli.minSupportedVersion is set to ${min_supported_version} for installer compatibility gates.
Confidence: high
Scope-risk: narrow
Directive: Keep package.json version, awikiCli.minSupportedVersion, and the awiki-cli Cargo package version aligned when cutting releases.
Tested: ${tested_summary}
Not-tested: ${not_tested_summary}
EOF
  fi
}

push_branch_if_needed() {
  local branch
  local upstream

  branch="$(git rev-parse --abbrev-ref HEAD)"
  if [[ "${branch}" == "HEAD" ]]; then
    die "currently on a detached HEAD; checkout a branch before releasing"
  fi

  upstream="$(git rev-parse --abbrev-ref --symbolic-full-name '@{u}' 2>/dev/null || true)"
  if [[ -z "${upstream}" ]]; then
    echo "Current branch has no upstream; pushing ${branch} to origin."
    git push -u origin "${branch}"
    return
  fi

  if [[ -n "$(git cherry)" ]]; then
    echo "Pushing local commits on ${branch} to ${upstream}."
    git push
  else
    echo "Branch ${branch} is already pushed to ${upstream}."
  fi
}

create_release_tag() {
  local channel="$1"
  local dist_tag="$2"

  if [[ "${channel}" == "stable" ]]; then
    scripts/release/release-tag-stable.sh
  else
    scripts/release/release-tag-prerelease.sh "${dist_tag}"
  fi
}

github_headers() {
  printf '%s\n' \
    "-H" "Accept: application/vnd.github+json" \
    "-H" "X-GitHub-Api-Version: 2022-11-28"
  if [[ -n "${GITHUB_TOKEN:-}" ]]; then
    printf '%s\n' "-H" "Authorization: Bearer ${GITHUB_TOKEN}"
  fi
}

wait_for_github_release_assets() {
  local tag="$1"
  local timeout="${AWIKI_RELEASE_GITHUB_TIMEOUT_SECONDS:-3600}"
  local deadline=$(( $(date +%s) + timeout ))
  local release_json
  local status
  local asset_count
  local header_args=()

  release_json="$(mktemp "${TMPDIR:-/tmp}/awiki-github-release.XXXXXX")"
  while IFS= read -r item; do
    header_args+=("${item}")
  done < <(github_headers)

  echo "Waiting for GitHub release assets for ${tag}..."
  while (( $(date +%s) < deadline )); do
    status="$(curl -sS -L -o "${release_json}" -w '%{http_code}' \
      "${header_args[@]}" \
      "https://api.github.com/repos/${GITHUB_OWNER}/${GITHUB_REPO}/releases/tags/${tag}" || true)"

    if [[ "${status}" == "200" ]]; then
      asset_count="$(json_value "${release_json}" 'Array.isArray(data.assets) ? data.assets.length : 0')"
      if (( asset_count > 0 )); then
        echo "GitHub release ${tag} is ready with ${asset_count} assets."
        rm -f "${release_json}"
        return
      fi
      echo "GitHub release ${tag} exists but has no assets yet; waiting..."
    else
      echo "GitHub release ${tag} is not ready yet (HTTP ${status}); waiting..."
    fi
    sleep 20
  done

  rm -f "${release_json}"
  die "timed out waiting for GitHub release assets for ${tag}"
}

wait_for_github_actions_or_assets() {
  local tag="$1"
  local timeout="${AWIKI_RELEASE_GITHUB_TIMEOUT_SECONDS:-3600}"
  local deadline=$(( $(date +%s) + timeout ))
  local run_id
  local run_status
  local run_conclusion
  local run_mode
  local runs_json

  if [[ "${WAIT_GITHUB}" != "1" ]]; then
    echo "Skipping GitHub workflow wait (--skip-wait)."
    return
  fi

  if command -v gh >/dev/null 2>&1; then
    echo "Looking for GitHub Actions release workflow run for ${tag}..."
    while (( $(date +%s) < deadline )); do
      if runs_json="$(gh run list --limit 50 \
        --json databaseId,headBranch,status,conclusion,workflowName,createdAt 2>/dev/null)"; then
        read -r run_mode run_id run_status run_conclusion < <(RUN_TAG="${tag}" node -e '
const fs = require("fs");
const tag = process.env.RUN_TAG;
const runs = JSON.parse(fs.readFileSync(0, "utf8"));
const releaseRuns = runs.filter(item => item.headBranch === tag && item.workflowName === "Release");
const activeRun = releaseRuns.find(item => item.status && item.status !== "completed");
const successfulRun = releaseRuns.find(item => item.status === "completed" && item.conclusion === "success");
const fallbackRun = releaseRuns[0];
let mode = "none";
let run = null;
if (activeRun) {
  mode = "watch";
  run = activeRun;
} else if (successfulRun) {
  mode = "assets";
  run = successfulRun;
} else if (fallbackRun) {
  mode = "assets";
  run = fallbackRun;
}
if (run && run.databaseId) {
  console.log([
    mode,
    String(run.databaseId),
    run.status || "unknown",
    run.conclusion || "none",
  ].join(" "));
} else {
  console.log("none none unknown none");
}
' <<<"${runs_json}")
        if [[ "${run_mode}" == "watch" && -n "${run_id}" && "${run_id}" != "null" ]]; then
          echo "Waiting for GitHub Actions run ${run_id}..."
          if ! gh run watch "${run_id}" --interval 15 --exit-status; then
            echo "Warning: GitHub Actions run ${run_id} ended with '${run_conclusion}'; checking whether release assets are already available." >&2
          fi
          wait_for_github_release_assets "${tag}"
          return
        fi
        if [[ "${run_mode}" == "assets" && -n "${run_id}" && "${run_id}" != "null" ]]; then
          if [[ "${run_conclusion}" == "success" ]]; then
            echo "Found completed successful GitHub Actions run ${run_id}; checking release assets."
          else
            echo "Latest visible release run ${run_id} completed with '${run_conclusion}'; checking whether release assets are already available."
          fi
          wait_for_github_release_assets "${tag}"
          return
        fi
      fi
      echo "Release workflow run for ${tag} not visible yet; waiting..."
      sleep 10
    done

    echo "Warning: could not find a GitHub Actions run for ${tag}; falling back to release asset polling." >&2
  else
    echo "GitHub CLI not found; falling back to release asset polling."
  fi

  wait_for_github_release_assets "${tag}"
}

mirror_to_gitee() {
  local tag="$1"

  if [[ "${PUBLISH_GITEE}" != "1" ]]; then
    echo "Skipping Gitee mirroring (--skip-gitee)."
    return
  fi

  if [[ -z "${GITEE_TOKEN:-}" ]]; then
    die "GITEE_TOKEN is required for Gitee mirroring; set it in scripts/release/release.env.local or use --skip-gitee"
  fi

  export GITHUB_OWNER GITHUB_REPO GITEE_OWNER GITEE_REPO GITEE_GIT_URL
  scripts/release/publish-gitee-release.sh "${tag}"
}

create_npm_userconfig() {
  local npmrc
  npmrc="$(mktemp "${TMPDIR:-/tmp}/awiki-npmrc.XXXXXX")"
  chmod 0600 "${npmrc}"
  printf '//registry.npmjs.org/:_authToken=%s\n' "${NODE_AUTH_TOKEN}" > "${npmrc}"
  printf '%s\n' "${npmrc}"
}

npm_version_exists() {
  local version="$1"
  npm view "${NPM_PACKAGE}@${version}" version --registry=https://registry.npmjs.org/ >/dev/null 2>&1
}

publish_to_npm() {
  local version="$1"
  local dist_tag="$2"
  local npmrc=""

  if [[ "${PUBLISH_NPM}" != "1" ]]; then
    echo "Skipping npm publish (--skip-npm)."
    return
  fi

  if npm_version_exists "${version}"; then
    echo "npm package ${NPM_PACKAGE}@${version} already exists; publish step skipped."
  else
    if [[ -z "${NODE_AUTH_TOKEN:-}" ]]; then
      die "NODE_AUTH_TOKEN is required to publish ${NPM_PACKAGE}@${version}; set it in scripts/release/release.env.local or use --skip-npm"
    fi

    npmrc="$(create_npm_userconfig)"
    NPM_USERCONFIG_TO_CLEAN="${npmrc}"

    echo "Verifying npm token..."
    npm --userconfig "${npmrc}" whoami --registry=https://registry.npmjs.org/ >/dev/null

    echo "Publishing ${NPM_PACKAGE}@${version} to npm with dist-tag ${dist_tag}..."
    npm --userconfig "${npmrc}" publish --access public --tag "${dist_tag}"
    rm -f "${npmrc}"
    NPM_USERCONFIG_TO_CLEAN=""
  fi

  wait_for_npm "${version}" "${dist_tag}"
}

wait_for_npm() {
  local version="$1"
  local dist_tag="$2"
  local timeout="${AWIKI_RELEASE_NPM_TIMEOUT_SECONDS:-900}"
  local deadline=$(( $(date +%s) + timeout ))
  local visible_version
  local tag_version

  echo "Waiting for npm registry visibility for ${NPM_PACKAGE}@${version}..."
  while (( $(date +%s) < deadline )); do
    visible_version="$(npm view "${NPM_PACKAGE}@${version}" version --registry=https://registry.npmjs.org/ 2>/dev/null || true)"
    if [[ "${visible_version}" == "${version}" ]]; then
      echo "npm version ${NPM_PACKAGE}@${version} is visible."
      tag_version="$(npm view "${NPM_PACKAGE}@${dist_tag}" version --registry=https://registry.npmjs.org/ 2>/dev/null || true)"
      if [[ "${tag_version}" == "${version}" ]]; then
        echo "npm dist-tag ${dist_tag} points to ${version}."
      else
        echo "Warning: npm dist-tag ${dist_tag} currently resolves to ${tag_version:-<missing>}." >&2
      fi
      return
    fi
    sleep 15
  done

  die "timed out waiting for npm package ${NPM_PACKAGE}@${version}"
}

VERSION=""
CHANNEL=""
DIST_TAG=""
MIN_SUPPORTED_VERSION=""
ENV_FILE="${AWIKI_RELEASE_ENV_FILE:-${ROOT_DIR}/scripts/release/release.env.local}"
RUN_TESTS=1
PUBLISH_GITEE=1
PUBLISH_NPM=1
WAIT_GITHUB=1
AUTO_COMMIT=1
DISABLE_PROXY=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --channel)
      CHANNEL="${2:-}"
      [[ -n "${CHANNEL}" ]] || die "--channel requires a value"
      shift 2
      ;;
    --dist-tag)
      DIST_TAG="${2:-}"
      [[ -n "${DIST_TAG}" ]] || die "--dist-tag requires a value"
      shift 2
      ;;
    --min-supported-version)
      MIN_SUPPORTED_VERSION="${2:-}"
      [[ -n "${MIN_SUPPORTED_VERSION}" ]] || die "--min-supported-version requires a value"
      shift 2
      ;;
    --env-file)
      ENV_FILE="${2:-}"
      [[ -n "${ENV_FILE}" ]] || die "--env-file requires a value"
      shift 2
      ;;
    --skip-tests)
      RUN_TESTS=0
      shift
      ;;
    --skip-gitee)
      PUBLISH_GITEE=0
      shift
      ;;
    --skip-npm)
      PUBLISH_NPM=0
      shift
      ;;
    --skip-wait)
      WAIT_GITHUB=0
      shift
      ;;
    --no-commit)
      AUTO_COMMIT=0
      shift
      ;;
    --no-proxy)
      DISABLE_PROXY=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --*)
      die "unknown option: $1"
      ;;
    *)
      if [[ -n "${VERSION}" ]]; then
        die "unexpected extra argument: $1"
      fi
      VERSION="$1"
      shift
      ;;
  esac
done

load_env_file "${ENV_FILE}"

require_command git
require_command node
require_command curl
require_command npm

if [[ "${DISABLE_PROXY}" == "1" ]]; then
  AWIKI_RELEASE_PROXY_URL=""
fi
configure_proxy

VERSION="${VERSION:-${AWIKI_RELEASE_VERSION:-$(read_package_version)}}"
[[ -n "${VERSION}" ]] || die "version is required and package.json.version is empty"
MIN_SUPPORTED_VERSION="${MIN_SUPPORTED_VERSION:-${AWIKI_RELEASE_MIN_SUPPORTED_VERSION:-${VERSION}}}"
ensure_semverish "${VERSION}"
ensure_semverish "${MIN_SUPPORTED_VERSION}"

if [[ -z "${CHANNEL}" ]]; then
  if [[ "${VERSION}" == *-* ]]; then
    CHANNEL="beta"
  else
    CHANNEL="stable"
  fi
fi

case "${CHANNEL}" in
  stable)
    [[ "${VERSION}" != *-* ]] || die "stable release version must not contain a pre-release suffix: ${VERSION}"
    DIST_TAG="${DIST_TAG:-latest}"
    ;;
  beta|prerelease)
    [[ "${VERSION}" == *-* ]] || die "pre-release version must contain a '-' suffix: ${VERSION}"
    CHANNEL="prerelease"
    if [[ -z "${DIST_TAG}" ]]; then
      DIST_TAG="${VERSION#*-}"
      DIST_TAG="${DIST_TAG%%.*}"
      DIST_TAG="${DIST_TAG:-beta}"
    fi
    ;;
  *)
    die "--channel must be stable, beta, or prerelease"
    ;;
esac

TAG="v${VERSION}"

cat <<EOF
Release plan:
  package: ${NPM_PACKAGE}
  version: ${VERSION}
  minSupportedVersion: ${MIN_SUPPORTED_VERSION}
  tag: ${TAG}
  channel: ${CHANNEL}
  npm dist-tag: ${DIST_TAG}
  gitee mirror: ${PUBLISH_GITEE}
  npm publish/verify: ${PUBLISH_NPM}
EOF

ensure_clean_worktree
update_package_json "${VERSION}" "${MIN_SUPPORTED_VERSION}"
update_cargo_manifest_version "${VERSION}"
cargo_bin="${CARGO:-cargo}"
toolchain="${AWIKI_CLI_RUST_TOOLCHAIN:-1.88.0}"
if [[ "${cargo_bin}" == "cargo" && -n "${toolchain}" ]]; then
  cargo_cmd=(cargo "+${toolchain}")
else
  cargo_cmd=("${cargo_bin}")
fi
"${cargo_cmd[@]}" generate-lockfile
"${cargo_cmd[@]}" run -p xtask -- check-version --expect "${VERSION}"

if [[ "${AUTO_COMMIT}" != "1" ]]; then
  echo "Updated package.json, Cargo.toml, and Cargo.lock only because --no-commit was set."
  echo "Review and commit the change, then run without --no-commit to publish."
  exit 0
fi

run_tests
commit_package_change "${VERSION}" "${MIN_SUPPORTED_VERSION}"
push_branch_if_needed
create_release_tag "${CHANNEL}" "${DIST_TAG}"
wait_for_github_actions_or_assets "${TAG}"
mirror_to_gitee "${TAG}"
publish_to_npm "${VERSION}" "${DIST_TAG}"

cat <<EOF

Release complete.
GitHub Release: https://github.com/${GITHUB_OWNER}/${GITHUB_REPO}/releases/tag/${TAG}
Gitee Release: https://gitee.com/${GITEE_OWNER}/${GITEE_REPO}/releases/tag/${TAG}
npm package: ${NPM_PACKAGE}@${VERSION}

Install checks:
  npm install -g ${NPM_PACKAGE}@${VERSION}
  awiki-cli version
EOF
