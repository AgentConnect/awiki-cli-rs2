#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SERVER_CONFIG="${SCRIPT_DIR}/publish-server.toml"

usage() { echo "Usage: $0 [--config FILE]"; }
while [[ $# -gt 0 ]]; do
  case "$1" in
    --config) SERVER_CONFIG="${2:-}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Error: unknown argument $1" >&2; usage >&2; exit 2 ;;
  esac
done

[[ -f "${SERVER_CONFIG}" ]] || { echo "Error: missing server config ${SERVER_CONFIG}" >&2; exit 1; }
mode="$(stat -c '%a' "${SERVER_CONFIG}" 2>/dev/null || stat -f '%Lp' "${SERVER_CONFIG}")"
[[ "${mode}" == "600" || "${mode}" == "400" ]] || {
  echo "Error: ${SERVER_CONFIG} must have mode 0600 or 0400 (got ${mode})" >&2
  exit 1
}

cfg() { node "${SCRIPT_DIR}/config.js" server "${SERVER_CONFIG}" "$1"; }
NGINX_CONFIG="$(cfg nginx_config)"
NGINX_HTTP_SNIPPET="$(cfg nginx_http_snippet)"
NGINX_SNIPPET="$(cfg nginx_snippet)"
NGINX_BACKUP_ROOT="$(cfg nginx_backup_root)"

for command in node sudo cmp install grep mktemp stat; do
  command -v "${command}" >/dev/null || { echo "Error: ${command} is required" >&2; exit 1; }
done
[[ "${NGINX_HTTP_SNIPPET}" != "${NGINX_SNIPPET}" ]] || {
  echo "Error: nginx_http_snippet and nginx_snippet must be different files" >&2
  exit 1
}
sudo grep -Fq "include ${NGINX_SNIPPET};" "${NGINX_CONFIG}" || {
  echo "Error: ${NGINX_CONFIG} must include ${NGINX_SNIPPET} before deployment" >&2
  exit 1
}

tmp="$(mktemp -d /tmp/awiki-cli-nginx.XXXXXX)"
trap 'rm -rf "${tmp}"' EXIT
http_candidate="${tmp}/$(basename "${NGINX_HTTP_SNIPPET}")"
server_candidate="${tmp}/$(basename "${NGINX_SNIPPET}")"
node "${SCRIPT_DIR}/render-nginx-download-zones.js" "${SERVER_CONFIG}" >"${http_candidate}"
node "${SCRIPT_DIR}/render-nginx-snippet.js" "${SERVER_CONFIG}" >"${server_candidate}"

same_file() {
  sudo test -f "$2" && sudo cmp -s "$1" "$2"
}

if same_file "${http_candidate}" "${NGINX_HTTP_SNIPPET}" \
    && same_file "${server_candidate}" "${NGINX_SNIPPET}"; then
  sudo nginx -t
  echo "config_changed=false"
  echo "reloaded=false"
  exit 0
fi

timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
backup_dir="${NGINX_BACKUP_ROOT}/cli-${timestamp}-$$"
sudo install -d -o root -g root -m 0700 "${backup_dir}"
http_existed=false
server_existed=false
if sudo test -f "${NGINX_HTTP_SNIPPET}"; then
  sudo cp -a "${NGINX_HTTP_SNIPPET}" "${backup_dir}/http-snippet.before"
  http_existed=true
fi
if sudo test -f "${NGINX_SNIPPET}"; then
  sudo cp -a "${NGINX_SNIPPET}" "${backup_dir}/server-snippet.before"
  server_existed=true
fi
sudo nginx -T 2>&1 | sudo tee "${backup_dir}/nginx-T.before.txt" >/dev/null
printf 'nginx_http_snippet=%s\nnginx_http_snippet_existed=%s\nnginx_snippet=%s\nnginx_snippet_existed=%s\n' \
  "${NGINX_HTTP_SNIPPET}" "${http_existed}" "${NGINX_SNIPPET}" "${server_existed}" \
  | sudo tee "${backup_dir}/state.txt" >/dev/null
backup_files=("${backup_dir}/nginx-T.before.txt" "${backup_dir}/state.txt")
[[ "${http_existed}" == "true" ]] && backup_files+=("${backup_dir}/http-snippet.before")
[[ "${server_existed}" == "true" ]] && backup_files+=("${backup_dir}/server-snippet.before")
sudo chown root:root "${backup_files[@]}"
sudo chmod 0600 "${backup_files[@]}"

reload_attempted=false
restore_previous() {
  local rc="${1:-1}"
  trap - ERR INT TERM
  echo "Error: Nginx deployment failed; restoring ${backup_dir}" >&2
  if [[ "${http_existed}" == "true" ]]; then
    sudo cp -a "${backup_dir}/http-snippet.before" "${NGINX_HTTP_SNIPPET}"
  else
    sudo rm -f "${NGINX_HTTP_SNIPPET}"
  fi
  if [[ "${server_existed}" == "true" ]]; then
    sudo cp -a "${backup_dir}/server-snippet.before" "${NGINX_SNIPPET}"
  else
    sudo rm -f "${NGINX_SNIPPET}"
  fi
  sudo nginx -t
  if [[ "${reload_attempted}" == "true" ]]; then
    sudo systemctl reload nginx
  fi
  exit "${rc}"
}
trap 'restore_previous $?' ERR
trap 'restore_previous 130' INT TERM

sudo install -d -o root -g root -m 0755 "$(dirname "${NGINX_HTTP_SNIPPET}")"
sudo install -d -o root -g root -m 0755 "$(dirname "${NGINX_SNIPPET}")"
sudo install -o root -g root -m 0644 "${http_candidate}" "${NGINX_HTTP_SNIPPET}"
sudo install -o root -g root -m 0644 "${server_candidate}" "${NGINX_SNIPPET}"
sudo nginx -t
sudo sha256sum "${NGINX_HTTP_SNIPPET}" "${NGINX_SNIPPET}" \
  | sudo tee "${backup_dir}/deployed-checksums.txt" >/dev/null
sudo chown root:root "${backup_dir}/deployed-checksums.txt"
sudo chmod 0600 "${backup_dir}/deployed-checksums.txt"
reload_attempted=true
sudo systemctl reload nginx
sudo systemctl is-active --quiet nginx
trap - ERR INT TERM

echo "config_changed=true"
echo "backup=${backup_dir}"
echo "reloaded=true"
