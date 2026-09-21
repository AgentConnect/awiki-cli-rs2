"""Offline release compatibility probe; temporary HOME, fake HTTP, no real credentials."""

import argparse
import json
import os
import pathlib
import queue
import signal
import subprocess
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

p = argparse.ArgumentParser()
p.add_argument("--python", required=True)
args = p.parse_args()
models = []


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_GET(self):
        body = json.dumps(
            {
                "object": "list",
                "data": [
                    {"id": m, "object": "model", "owned_by": "deepseek"}
                    for m in ["deepseek-v4-pro", "deepseek-flash"]
                ],
            }
        ).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        req = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if "model" not in req:
            self.send_response(404)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        model = req["model"]
        models.append(model)
        if req.get("stream"):
            chunks = [
                {
                    "id": "offline",
                    "object": "chat.completion.chunk",
                    "created": 1,
                    "model": model,
                    "choices": [
                        {
                            "index": 0,
                            "delta": {"role": "assistant", "content": "OFFLINE_OK"},
                            "finish_reason": None,
                        }
                    ],
                },
                {
                    "id": "offline",
                    "object": "chat.completion.chunk",
                    "created": 1,
                    "model": model,
                    "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
                    "usage": {
                        "prompt_tokens": 1,
                        "completion_tokens": 1,
                        "total_tokens": 2,
                    },
                },
            ]
            body = (
                "".join("data: " + json.dumps(c) + "\n\n" for c in chunks)
                + "data: [DONE]\n\n"
            ).encode()
            typ = "text/event-stream"
        else:
            body = json.dumps(
                {
                    "id": "offline",
                    "object": "chat.completion",
                    "created": 1,
                    "model": model,
                    "choices": [
                        {
                            "index": 0,
                            "message": {"role": "assistant", "content": "OFFLINE_OK"},
                            "finish_reason": "stop",
                        }
                    ],
                    "usage": {
                        "prompt_tokens": 1,
                        "completion_tokens": 1,
                        "total_tokens": 2,
                    },
                }
            ).encode()
            typ = "application/json"
        self.send_response(200)
        self.send_header("Content-Type", typ)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
bootstrap = """
import socket,runpy,sys
connect=socket.socket.connect
connect_ex=socket.socket.connect_ex
getaddrinfo=socket.getaddrinfo
def allowed(address):
 if not isinstance(address,tuple) or address[0] not in ('127.0.0.1','::1','localhost'):raise OSError('offline probe blocks external network')
def safe_connect(s,address):
 allowed(address);return connect(s,address)
def safe_connect_ex(s,address):
 allowed(address);return connect_ex(s,address)
def safe_resolve(host,*a,**kw):
 if host not in ('127.0.0.1','::1','localhost',None):raise OSError('offline probe blocks external DNS')
 return getaddrinfo(host,*a,**kw)
socket.socket.connect=safe_connect;socket.socket.connect_ex=safe_connect_ex;socket.getaddrinfo=safe_resolve
sys.argv=['hermes-acp'];runpy.run_module('acp_adapter.entry',run_name='__main__')
"""
with tempfile.TemporaryDirectory(prefix="hermes-model-contract-") as tmp:
    home = pathlib.Path(tmp)
    hh = home / "hermes"
    hh.mkdir()
    (home / "work").mkdir()
    (hh / "config.yaml").write_text(
        "model:\n  provider: deepseek\n  default: deepseek-v4-pro\n  base_url: http://127.0.0.1:"
        + str(server.server_port)
        + "/v1\n  context_length: 64000\nagent:\n  max_turns: 2\nmemory:\n  memory_enabled: false\n  user_profile_enabled: false\ncompression:\n  enabled: false\n"
    )
    env = {
        "HOME": tmp,
        "HERMES_HOME": str(hh),
        "PATH": str(pathlib.Path(args.python).parent) + ":/usr/bin:/bin",
        "LANG": "en_US.UTF-8",
        "DEEPSEEK_API_KEY": "offline-fixture-only",
        "NO_PROXY": "127.0.0.1,localhost",
        "PYTHONDONTWRITEBYTECODE": "1",
        "HERMES_DISABLE_LAZY_INSTALLS": "1",
    }

    def start():
        log = open(home / "stderr.log", "a")
        proc = subprocess.Popen(
            [args.python, "-c", bootstrap],
            cwd=home,
            env=env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=log,
            text=True,
            start_new_session=True,
        )
        q = queue.Queue()

        def reader():
            for line in proc.stdout:
                try:
                    q.put(json.loads(line))
                except json.JSONDecodeError:
                    q.put({"bad_output": line})
            q.put({"closed": proc.poll()})

        threading.Thread(target=reader, daemon=True).start()
        return proc, q, log

    def stop(proc, log):
        try:
            os.killpg(proc.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(proc.pid, signal.SIGKILL)
            proc.wait()
        log.close()

    counter = 0

    def rpc(proc, q, method, params):
        global counter
        counter += 1
        rid = counter
        proc.stdin.write(
            json.dumps(
                {"jsonrpc": "2.0", "id": rid, "method": method, "params": params}
            )
            + "\n"
        )
        proc.stdin.flush()
        deadline = time.monotonic() + 60
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError(method)
            r = q.get(timeout=remaining)
            if "closed" in r or "bad_output" in r:
                raise RuntimeError(r)
            if r.get("id") == rid:
                if "error" in r:
                    raise RuntimeError(r["error"])
                return r["result"]
            if "id" in r and "method" in r:
                raise RuntimeError("Unexpected interaction " + r["method"])

    def init(proc, q):
        return rpc(
            proc,
            q,
            "initialize",
            {
                "protocolVersion": 1,
                "clientCapabilities": {},
                "clientInfo": {"name": "offline-check", "version": "1"},
            },
        )

    try:
        proc, q, log = start()
        try:
            init(proc, q)
            s = rpc(
                proc, q, "session/new", {"cwd": str(home / "work"), "mcpServers": []}
            )
            sid = s["sessionId"]
            assert s["models"]["currentModelId"] == "deepseek:deepseek-v4-pro"
            rpc(
                proc,
                q,
                "session/prompt",
                {
                    "sessionId": sid,
                    "prompt": [{"type": "text", "text": "Reply OFFLINE_OK"}],
                },
            )
            assert models[-1] == "deepseek-v4-pro", models
            rpc(
                proc,
                q,
                "session/set_model",
                {"sessionId": sid, "modelId": "deepseek:deepseek-flash"},
            )
            rpc(
                proc,
                q,
                "session/prompt",
                {
                    "sessionId": sid,
                    "prompt": [{"type": "text", "text": "Reply OFFLINE_OK"}],
                },
            )
            assert models[-1] == "deepseek-flash", models
        finally:
            stop(proc, log)
        proc, q, log = start()
        try:
            init(proc, q)
            s = rpc(
                proc,
                q,
                "session/load",
                {"sessionId": sid, "cwd": str(home / "work"), "mcpServers": []},
            )
            assert s["models"]["currentModelId"] == "deepseek:deepseek-flash"
            rpc(
                proc,
                q,
                "session/prompt",
                {
                    "sessionId": sid,
                    "prompt": [{"type": "text", "text": "Reply OFFLINE_OK"}],
                },
            )
            assert models[-1] == "deepseek-flash", models
        finally:
            stop(proc, log)
        print(
            json.dumps(
                {"passed": True, "request_models": models, "real_model_calls": 0}
            )
        )
    except BaseException:
        print((home / "stderr.log").read_text()[-10000:])
        raise
server.shutdown()
server.server_close()
