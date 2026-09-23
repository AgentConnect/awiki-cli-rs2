#!/usr/bin/env python3
"""Build once, then run ACP contracts without user config, Agent CLIs or API keys."""
import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def test_environment(home, tool_dir):
    # An allowlist keeps future provider credentials and user CLI configuration out.
    return {
        "HOME": str(home),
        "TMPDIR": str(home),
        "XDG_CONFIG_HOME": str(home / "config"),
        "XDG_DATA_HOME": str(home / "data"),
        "XDG_CACHE_HOME": str(home / "cache"),
        "PATH": str(tool_dir),
        "LANG": "en_US.UTF-8",
        "NO_PROXY": "127.0.0.1,localhost",
        "PYTHONDONTWRITEBYTECODE": "1",
        # Keep the bounded host Node probe independent of other ACP tests.
        "RUST_TEST_THREADS": "1",
    }


def test_binary(output, target="awiki_deamon"):
    for line in output.splitlines():
        record = json.loads(line)
        if (record.get("reason") == "compiler-artifact"
                and record.get("target", {}).get("name") == target
                and record.get("profile", {}).get("test")
                and record.get("executable")):
            return record["executable"]
    raise RuntimeError("Cargo did not produce test executable: " + target)


def run_selection(executable, selection, environment):
    listed = subprocess.check_output([executable, selection, "--list"], cwd=ROOT,
                                     env=environment, text=True, timeout=10)
    if not any(line.endswith(": test") for line in listed.splitlines()):
        raise RuntimeError("No tests matched " + selection)
    process = subprocess.Popen([executable, selection], cwd=ROOT,
                               env=environment, start_new_session=True)
    try:
        return process.wait(timeout=180)
    finally:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cargo-toolchain", default=os.environ.get("AWIKI_DAEMON_RUST_CARGO_TOOLCHAIN"))
    args = parser.parse_args()
    if sys.platform not in ("darwin", "linux"):
        parser.error("ACP subprocess contracts support macOS and Linux")
    binaries = {name: shutil.which(name) for name in ("cargo", "node", "python3", "ps", "sleep")}
    missing = [name for name, path in binaries.items() if not path]
    if missing:
        parser.error("Missing development tools: " + ", ".join(missing))
    node_version = subprocess.check_output([binaries["node"], "-p", "process.versions.node"], text=True)
    if tuple(map(int, node_version.strip().split(".")[:2])) < (20, 6):
        parser.error("Node.js 20.6 or newer is required for the Gemini protocol fixtures")
    command = [binaries["cargo"]]
    if args.cargo_toolchain:
        command.append("+" + args.cargo_toolchain)
    command += ["test", "--locked", "-p", "awiki-deamon", "--lib", "--test", "agent_registration_management", "--test", "acp_routing_contracts", "--test", "acp_subprocess_contracts", "--no-run", "--message-format=json"]
    # Compilation uses normal dependency caches. Only the resulting test process
    # receives the isolated environment; no model calls happen during compilation.
    built = subprocess.run(command, cwd=ROOT, stdout=subprocess.PIPE, text=True, timeout=1800)
    if built.returncode:
        return built.returncode
    executable = test_binary(built.stdout)
    integrations = [test_binary(built.stdout, name) for name in ("agent_registration_management", "acp_routing_contracts", "acp_subprocess_contracts")]
    with tempfile.TemporaryDirectory(prefix="acp-contract-") as temporary:
        home = Path(temporary)
        tool_dir = home / "bin"
        tool_dir.mkdir()
        for name in ("node", "python3", "ps", "sleep"):
            (tool_dir / name).symlink_to(binaries[name])
        environment = test_environment(home, tool_dir)
        for selection in ("acp", "group_context::tests", "runtime_clients", "cli_runtime_env::tests", "agent_status::tests", "runtime::host::tests", "state::runtime_retirement", "inbox::user_delegated::tests", "app_bridge::action::tests", "cli_wrapper::input_tests", "foreground::tests"):
            result = run_selection(executable, selection, environment)
            if result:
                return result
        for integration in integrations:
            result = run_selection(integration, "", environment)
            if result:
                return result
    return 0


if __name__ == "__main__":
    sys.exit(main())
