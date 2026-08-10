#!/bin/sh
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
TEMPLATE="${SCRIPT_DIR}/../_cleanup.sh.template"
TEMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/awiki-cleanup-test.XXXXXX")"
trap 'rm -rf "$TEMP_ROOT"' EXIT HUP INT TERM

HOME_DIR="${TEMP_ROOT}/home"
FAKE_BIN="${TEMP_ROOT}/bin"
mkdir -p "${HOME_DIR}/.awiki-daemon/deamon" "$FAKE_BIN"
printf 'test-data\n' >"${HOME_DIR}/.awiki-daemon/deamon/state.db"

cat >"${FAKE_BIN}/uname" <<'EOF'
#!/bin/sh
printf 'AWikiCleanupContractTestOS\n'
EOF
chmod +x "${FAKE_BIN}/uname"

assert_contains() {
  haystack="$1"
  needle="$2"
  case "$haystack" in
    *"$needle"*) ;;
    *)
      printf 'expected cleanup output to contain: %s\n' "$needle" >&2
      exit 1
      ;;
  esac
}

help_output="$(HOME="$HOME_DIR" sh "$TEMPLATE" --help 2>&1)"
assert_contains "$help_output" "local host cleanup only"
assert_contains "$help_output" "remove the corresponding offline Daemon from your account"

dry_run_output="$(PATH="${FAKE_BIN}:$PATH" HOME="$HOME_DIR" sh "$TEMPLATE" --dry-run --yes 2>&1)"
assert_contains "$dry_run_output" "no local data or AWiki account state was changed"
if [ ! -e "${HOME_DIR}/.awiki-daemon/deamon/state.db" ]; then
  printf 'expected dry-run to preserve local daemon product data\n' >&2
  exit 1
fi

cleanup_output="$(PATH="${FAKE_BIN}:$PATH" HOME="$HOME_DIR" sh "$TEMPLATE" --yes 2>&1)"
assert_contains "$cleanup_output" "AWiki daemon host cleanup complete"
assert_contains "$cleanup_output" "Only data on this host was removed"
assert_contains "$cleanup_output" "were not removed from your AWiki account"
assert_contains "$cleanup_output" "choose 'Remove from account' to finish"

if [ -e "${HOME_DIR}/.awiki-daemon/deamon" ]; then
  printf 'expected local daemon product data to be removed\n' >&2
  exit 1
fi

printf 'cleanup script contract passed\n'
