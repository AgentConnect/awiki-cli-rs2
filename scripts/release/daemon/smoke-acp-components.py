#!/usr/bin/env python3
"""Initialize packaged upstream adapters using a fake native CLI and empty HOME.

This is an artifact smoke check, not model acceptance. It needs a prepared bundle
and Python, but never a host Agent CLI, model API key or user configuration.
"""
import argparse
import json
import os
from pathlib import Path
import selectors
import signal
import subprocess
import sys
import tempfile

NATIVE_FIXTURE = '''import json, sys
from pathlib import Path
if '--version' in sys.argv:
    print('codex-cli 0.154.0')
    raise SystemExit(0)
for line in sys.stdin:
    request = json.loads(line)
    with Path(__file__).with_suffix('.methods').open('a') as log:
        log.write(str(request.get('method')) + '\\n')
    if 'id' not in request:
        continue
    if request.get('method') != 'initialize':
        raise SystemExit('Only initialization is permitted in the artifact smoke fixture')
    print(json.dumps({'jsonrpc':'2.0','id':request['id'],'result':{
        'userAgent':'codex/0.154.0','platformFamily':'unix','platformOs':sys.platform}}), flush=True)
'''


def smoke(bundle):
    manifest = json.loads((bundle / "manifest.json").read_text())
    if not manifest["available"]:
        raise ValueError("This artifact has no runnable adapter components")
    version = subprocess.check_output([str(bundle / "node"), "--version"], text=True).strip()
    if version != "v" + manifest["node_version"]:
        raise ValueError("Unexpected packaged Node version")
    with tempfile.TemporaryDirectory(prefix="acp-package-smoke-", dir=bundle.parent) as temporary:
        root = Path(temporary)
        native = root / "native-fixture"
        native.write_text("#!" + sys.executable + "\n" + NATIVE_FIXTURE)
        native.chmod(0o700)
        for name, adapter in manifest["adapters"].items():
            environment = {"HOME": str(root), "PATH": "/usr/bin:/bin", "LANG": "en_US.UTF-8",
                           "CODEX_HOME": str(root / "codex"), "CLAUDE_CONFIG_DIR": str(root / "claude"),
                           "XDG_CONFIG_HOME": str(root / "config"), "XDG_DATA_HOME": str(root / "data"),
                           "XDG_CACHE_HOME": str(root / "cache"), adapter["executable_env"]: str(native)}
            with (root / f"{name}.stderr").open("w+b") as errors:
                process = subprocess.Popen([str(bundle / "node"), str(bundle / adapter["entry"])],
                                           stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=errors,
                                           env=environment, cwd=root, start_new_session=True)
                try:
                    request = {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                        "protocolVersion": 1, "clientCapabilities": {},
                        "clientInfo": {"name": "awiki-artifact-smoke", "version": "1"}}}
                    process.stdin.write((json.dumps(request) + "\n").encode())
                    process.stdin.flush()
                    with selectors.DefaultSelector() as selector:
                        selector.register(process.stdout, selectors.EVENT_READ)
                        if not selector.select(15):
                            raise RuntimeError(f"{name}: initialization timed out")
                        response = json.loads(process.stdout.readline())
                    result = response.get("result", {})
                    if (response.get("id") != 1 or result.get("protocolVersion") != 1
                            or result.get("agentInfo", {}).get("version") != adapter["version"]):
                        raise RuntimeError(f"{name}: initialization failed")
                    if name == "codex" and native.with_suffix(".methods").read_text().splitlines() != ["initialize"]:
                        raise RuntimeError("Codex did not use the isolated native App Server fixture")
                    print(json.dumps({"adapter": name, "version": adapter["version"], "initialize": "passed", "model_calls": 0}))
                finally:
                    try:
                        os.killpg(process.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    process.wait()
                    process.stdin.close()
                    process.stdout.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, required=True)
    smoke(parser.parse_args().bundle.resolve())
