"""Verify the official loopback proxy's error status, without model requests.

Uses existing protected config/key files, an independent temporary process/port,
and the reviewed compatibility module. Never changes the running user relay.
"""
import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request


def verify(config_dir: Path):
    key = (config_dir / "relay-key").read_text().strip()
    environment = os.environ.copy()
    environment["DEEPSEEK_API_KEY"] = (Path.home() / ".config/deepseek/api-key").read_text().strip()
    environment["LITELLM_MASTER_KEY"] = key
    environment["LITELLM_LOG"] = "ERROR"
    environment["LITELLM_TELEMETRY"] = "False"
    environment["NO_PROXY"] = "127.0.0.1,localhost,api.deepseek.com"
    for name in list(environment):
        if name.lower() in {"http_proxy", "https_proxy", "all_proxy"}:
            del environment[name]
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]
    with tempfile.TemporaryDirectory(prefix="awiki-relay-check-") as folder:
        # Resolve the source module in the reviewed checkout, not in user config.
        script = Path(folder) / "serve.py"
        script.write_text("import sys\nsys.path.insert(0, " + repr(str(Path(__file__).resolve().parent)) + ")\n"
                          "from compatibility import install_proxy_error_handler\ninstall_proxy_error_handler()\n"
                          "from litellm.proxy.proxy_cli import run_server\nrun_server()\n")
        process = subprocess.Popen([sys.executable, str(script), "--host", "127.0.0.1", "--port", str(port),
                                    "--config", str(config_dir / "config.yaml")], env=environment,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            deadline = time.monotonic() + 40
            while True:
                if process.poll() is not None:
                    raise RuntimeError("temporary_relay_startup_failed")
                try:
                    with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                        break
                except OSError:
                    if time.monotonic() > deadline:
                        raise RuntimeError("temporary_relay_startup_timeout") from None
                    time.sleep(0.2)
            cases = []
            for endpoint in ["generateContent", "streamGenerateContent", "countTokens"]:
                started = time.monotonic()
                request = urllib.request.Request(
                    f"http://127.0.0.1:{port}/v1beta/models/deepseek-flash:{endpoint}",
                    data=json.dumps({"contents": [{"role": "user", "parts": [{"functionResponse": {
                        "id": "orphan", "name": "shell", "response": {"output": "fixture"},
                    }}]}]}).encode(), headers={"Content-Type": "application/json", "x-goog-api-key": key},
                )
                try:
                    with urllib.request.urlopen(request, timeout=15) as response:
                        status = response.status
                        body = response.read()
                except urllib.error.HTTPError as error:
                    status = error.code
                    body = error.read()
                seconds = round(time.monotonic() - started, 3)
                cases.append({"endpoint": endpoint, "status": status, "seconds": seconds,
                              "safe_code": b"gemini_invalid_history:ambiguous_or_orphan_tool_result" in body,
                              "pass": status == 400 and seconds < 10})
            return {"cases": cases, "pass": all(case["pass"] and case["safe_code"] for case in cases),
                    "model_requests": 0}
        finally:
            process.terminate()
            try:
                process.wait(timeout=8)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--config-dir', type=Path, default=Path.home() / '.config/gemini-deepseek')
    parser.add_argument('--report', type=Path)
    args = parser.parse_args()
    outcome = verify(args.config_dir)
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(outcome, indent=2) + '\n')
    print(json.dumps(outcome))
    sys.exit(0 if outcome['pass'] else 1)
