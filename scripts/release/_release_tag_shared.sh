#!/usr/bin/env bash

release_root_dir() {
  cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
}

release_require_command() {
  local cmd="$1"
  local script_name="$2"
  if ! command -v "${cmd}" >/dev/null 2>&1; then
    echo "Error: ${cmd} is required to run ${script_name}" >&2
    exit 1
  fi
}

release_read_version() {
  local root_dir="$1"
  if [[ ! -f "${root_dir}/package.json" ]]; then
    echo "Error: package.json not found in ${root_dir}" >&2
    exit 1
  fi

  local version
  version="$(node - "${root_dir}/package.json" <<'NODE'
const fs = require('fs');
const packagePath = process.argv[2];
const pkg = JSON.parse(fs.readFileSync(packagePath, 'utf8'));
process.stdout.write(typeof pkg.version === 'string' ? pkg.version.trim() : '');
NODE
)"
  if [[ -z "${version}" ]]; then
    echo "Error: .version is missing or empty in package.json" >&2
    exit 1
  fi

  printf '%s\n' "${version}"
}

release_require_clean_worktree() {
  if [[ -n "$(git status --porcelain)" ]]; then
    echo "Error: working tree is not clean; please commit or stash changes before tagging" >&2
    exit 1
  fi
}

release_require_branch_with_upstream() {
  local branch
  branch="$(git rev-parse --abbrev-ref HEAD)"
  if [[ "${branch}" == "HEAD" ]]; then
    echo "Error: currently on a detached HEAD; please checkout a branch before tagging" >&2
    exit 1
  fi

  if ! git rev-parse --abbrev-ref --symbolic-full-name '@{u}' >/dev/null 2>&1; then
    echo "Error: current branch ${branch} has no upstream; please set upstream and push before tagging" >&2
    exit 1
  fi

  if [[ -n "$(git cherry)" ]]; then
    echo "Error: there are local commits not pushed to origin; please push them before tagging" >&2
    exit 1
  fi

  printf '%s\n' "${branch}"
}

release_local_tag_commit() {
  local tag="$1"
  git rev-parse -q --verify "refs/tags/${tag}^{commit}" 2>/dev/null || true
}

release_remote_tag_commit() {
  local remote="$1"
  local tag="$2"
  local output

  output="$(git ls-remote --tags "${remote}" "refs/tags/${tag}^{}" "refs/tags/${tag}" 2>/dev/null || true)"
  if [[ -z "${output}" ]]; then
    return
  fi

  awk '
    $2 ~ /\^\{\}$/ { print $1; found=1; exit }
    !found && $2 ~ /^refs\/tags\// { fallback=$1 }
    END {
      if (!found && fallback != "") {
        print fallback
      }
    }
  ' <<<"${output}"
}

release_require_tag_absent() {
  local tag="$1"

  if git rev-parse -q --verify "refs/tags/${tag}" >/dev/null; then
    echo "Error: tag ${tag} already exists locally" >&2
    exit 1
  fi

  if git ls-remote --tags origin "refs/tags/${tag}" | grep -q .; then
    echo "Error: tag ${tag} already exists on origin" >&2
    exit 1
  fi
}

release_ensure_tag_on_remote() {
  local tag="$1"
  local message="$2"
  local remote="${3:-origin}"
  local head_commit
  local local_commit
  local remote_commit

  head_commit="$(git rev-parse HEAD)"
  local_commit="$(release_local_tag_commit "${tag}")"
  remote_commit="$(release_remote_tag_commit "${remote}" "${tag}")"

  if [[ -n "${local_commit}" && "${local_commit}" != "${head_commit}" ]]; then
    echo "Error: local tag ${tag} already exists but points to ${local_commit}, not current HEAD ${head_commit}" >&2
    exit 1
  fi

  if [[ -n "${remote_commit}" && "${remote_commit}" != "${head_commit}" ]]; then
    echo "Error: tag ${tag} already exists on ${remote} but points to ${remote_commit}, not current HEAD ${head_commit}" >&2
    exit 1
  fi

  if [[ -n "${local_commit}" && -n "${remote_commit}" ]]; then
    echo "Tag ${tag} already exists locally and on ${remote}; reusing it."
    return
  fi

  if [[ -z "${local_commit}" && -n "${remote_commit}" ]]; then
    echo "Tag ${tag} already exists on ${remote}; fetching it locally."
    git fetch "${remote}" "refs/tags/${tag}:refs/tags/${tag}"
    return
  fi

  if [[ -n "${local_commit}" && -z "${remote_commit}" ]]; then
    echo "Tag ${tag} already exists locally; pushing it to ${remote}."
    git push "${remote}" "refs/tags/${tag}:refs/tags/${tag}"
    return
  fi

  echo "Creating tag ${tag} on current HEAD and pushing to ${remote}..."
  git tag -a "${tag}" -m "${message}"
  git push "${remote}" "${tag}"
}

release_create_and_push_tag() {
  local tag="$1"
  local message="$2"

  git tag -a "${tag}" -m "${message}"
  git push origin "${tag}"
}
