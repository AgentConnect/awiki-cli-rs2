#!/usr/bin/env python3
"""仅在一次性 GitHub macOS/Windows runner 验证真实系统代理；不操作开发电脑。"""
from __future__ import annotations

import argparse
import contextlib
import ctypes
import hashlib
import http.server
import json
import os
from pathlib import Path
import platform
import queue
import re
import socket
import subprocess
import threading
import time

ADDRESS = "192.0.2.45"
CHANGE_ADDRESS = "192.0.2.46"


def require_disposable_runner(environment=None, system=None):
    environment = os.environ if environment is None else environment
    system = platform.system() if system is None else system
    if (environment.get("GITHUB_ACTIONS") != "true"
            or environment.get("RUNNER_ENVIRONMENT") != "github-hosted"
            or system not in {"Darwin", "Windows"}):
        raise RuntimeError("requires_disposable_github_macos_or_windows_runner")


def run(*args):
    result = subprocess.run(args, capture_output=True, text=True, timeout=30)
    if result.returncode:
        raise RuntimeError("native_command_failed:" + args[0])
    return result.stdout.strip()


class WindowsProxy:
    def __init__(self):
        from ctypes import wintypes as w
        class Value(ctypes.Union):
            _fields_ = [("number", w.DWORD), ("pointer", ctypes.c_void_p), ("time", w.FILETIME)]
        class Option(ctypes.Structure):
            _fields_ = [("option", w.DWORD), ("value", Value)]
        class Options(ctypes.Structure):
            _fields_ = [("size", w.DWORD), ("connection", w.LPWSTR), ("count", w.DWORD),
                        ("error", w.DWORD), ("options", ctypes.POINTER(Option))]
        self.w, self.Option, self.Options = w, Option, Options
        self.api = ctypes.WinDLL("wininet", use_last_error=True)
        self.api.InternetQueryOptionW.argtypes = [w.HANDLE, w.DWORD, ctypes.c_void_p, ctypes.POINTER(w.DWORD)]
        self.api.InternetSetOptionW.argtypes = [w.HANDLE, w.DWORD, ctypes.c_void_p, w.DWORD]
        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self.kernel.GlobalFree.argtypes = [ctypes.c_void_p]
        self.kernel.GlobalFree.restype = ctypes.c_void_p
        self.saved = self.query()

    def options(self):
        values = (self.Option * 4)()
        for item, key in zip(values, (1, 2, 3, 4)):
            item.option = key
        group = self.Options(ctypes.sizeof(self.Options), None, 4, 0, values)
        return values, group

    def query(self):
        values, group = self.options()
        size = self.w.DWORD(ctypes.sizeof(group))
        if not self.api.InternetQueryOptionW(None, 75, ctypes.byref(group), ctypes.byref(size)):
            raise RuntimeError("native_proxy_query_failed")
        strings = []
        for item in values[1:]:
            pointer = item.value.pointer
            strings.append(ctypes.wstring_at(pointer) if pointer else "")
            if pointer:
                self.kernel.GlobalFree(pointer)
        return (values[0].value.number, *strings)

    def apply(self, flags, proxy, bypass, pac):
        values, group = self.options()
        values[0].value.number = flags
        buffers = [ctypes.create_unicode_buffer(s) for s in (proxy, bypass, pac)]
        for item, buffer in zip(values[1:], buffers):
            item.value.pointer = ctypes.cast(buffer, ctypes.c_void_p)
        if not self.api.InternetSetOptionW(None, 75, ctypes.byref(group), ctypes.sizeof(group)):
            raise RuntimeError("native_proxy_update_failed")
        for option in (39, 37):
            if not self.api.InternetSetOptionW(None, option, None, 0):
                raise RuntimeError("native_proxy_refresh_failed")
        if self.query() != (flags, proxy, bypass, pac):
            raise RuntimeError("native_proxy_readback_mismatch")

    def set(self, port=None, bypass="", pac=""):
        self.apply(1 | (2 if port else 0) | (4 if pac else 0),
                   f"http=127.0.0.1:{port};https=127.0.0.1:{port}" if port else "", bypass, pac)

    def restore(self):
        self.apply(*self.saved)


class MacProxy:
    def __init__(self):
        interface = re.search(r"interface:\s*(\S+)", run("route", "-n", "get", "default")).group(1)
        order = run("networksetup", "-listnetworkserviceorder").splitlines()
        self.service = None
        for previous, line in zip(order, order[1:]):
            if f"Device: {interface})" in line:
                self.service = re.sub(r"^\(\d+\)\s*", "", previous)
        if not self.service:
            raise RuntimeError("default_network_service_missing")
        self.saved = {}
        for kind in ("webproxy", "securewebproxy", "autoproxyurl", "proxyautodiscovery"):
            value = run("networksetup", "-get" + kind, self.service)
            self.saved[kind] = dict(line.split(": ", 1) for line in value.splitlines() if ": " in line)
        bypass = run("networksetup", "-getproxybypassdomains", self.service)
        self.saved["bypass"] = [] if bypass.startswith("There aren't any") else bypass.splitlines()
        # No credentials are collected or changed. Unexpected preconfiguration aborts.
        for kind in ("webproxy", "securewebproxy"):
            if self.saved[kind].get("Authenticated Proxy Enabled") == "1":
                raise RuntimeError("authenticated_runner_proxy_not_supported")

    def command(self, option, *values):
        run("sudo", "-n", "networksetup", "-" + option, self.service, *map(str, values))

    def set(self, port=None, bypass="", pac=""):
        self.command("setproxyautodiscovery", "off")
        for kind in ("web", "secureweb"):
            if port:
                self.command("set" + kind + "proxy", "127.0.0.1", port)
            self.command("set" + kind + "proxystate", "on" if port else "off")
        if pac:
            self.command("setautoproxyurl", pac)
        self.command("setautoproxystate", "on" if pac else "off")
        self.command("setproxybypassdomains", *(bypass.split(";") if bypass else ["Empty"]))

    def restore(self):
        for kind in ("web", "secureweb"):
            saved = self.saved[kind + "proxy"]
            if saved.get("Server"):
                self.command("set" + kind + "proxy", saved["Server"], saved["Port"])
            self.command("set" + kind + "proxystate", "on" if saved.get("Enabled") == "Yes" else "off")
        pac = self.saved["autoproxyurl"]
        if pac.get("URL") and pac["URL"] != "(null)":
            self.command("setautoproxyurl", pac["URL"])
        self.command("setautoproxystate", "on" if pac.get("Enabled") == "Yes" else "off")
        discovery = self.saved["proxyautodiscovery"]
        self.command("setproxyautodiscovery", "on" if discovery.get("Auto Proxy Discovery") == "On" else "off")
        self.command("setproxybypassdomains", *(self.saved["bypass"] or ["Empty"]))


def alias(address, add):
    if platform.system() == "Darwin":
        run("sudo", "-n", "ifconfig", "lo0", "alias" if add else "-alias", address)
    else:
        # Index 1 is the Windows loopback; never assign a static IP to the DHCP NIC.
        args = ["netsh", "interface", "ipv4", "add" if add else "delete", "address", "name=1", f"address={address}"]
        if add:
            args += ["mask=255.255.255.255", "skipassource=true"]
        run(*args, "store=active")


class Fixture(http.server.ThreadingHTTPServer):
    daemon_threads = True
    def __init__(self, label):
        self.label, self.mode, self.pac = label, "ok", ""
        self.events = []
        self.stop = threading.Event()
        super().__init__(("0.0.0.0", 0), Handler)
        self.port = self.server_address[1]
        self.worker = threading.Thread(target=self.serve_forever, daemon=True)
        self.worker.start()

    def close(self):
        self.stop.set()
        self.shutdown()
        self.server_close()
        self.worker.join(timeout=3)


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def event(self):
        self.server.events.append({"method": self.command, "business": self.headers.get("x-awiki-fixture") is not None})

    def response(self, code, body, content_type="text/plain"):
        data = body.encode()
        self.send_response(code)
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Content-Type", content_type)
        self.end_headers()
        self.wfile.write(data)

    def do_CONNECT(self):
        self.event()
        if self.server.mode == "hang":
            self.server.stop.wait(25)
        elif self.server.mode == "auth":
            self.response(407, "proxy-auth-required")
        else:
            self.response(200, "")

    def do_GET(self):
        self.event()
        if self.path.startswith("/pac"):
            self.response(200, self.server.pac, "application/x-ns-proxy-autoconfig")
        else:
            self.response(403 if self.server.mode == "deny" else 200, self.server.label)

    def do_POST(self):
        self.rfile.read(int(self.headers.get("Content-Length", 0)))
        self.event()
        if self.server.mode == "drop-write":
            self.close_connection = True
            with contextlib.suppress(OSError):
                self.connection.shutdown(socket.SHUT_RDWR)
        else:
            self.response(200, self.server.label)


class Probe:
    def __init__(self, binary):
        environment = dict(os.environ)
        environment.update(HTTP_PROXY="http://127.0.0.1:9", HTTPS_PROXY="http://127.0.0.1:9",
                           ALL_PROXY="http://127.0.0.1:9", NO_PROXY="", http_proxy="http://127.0.0.1:9",
                           https_proxy="http://127.0.0.1:9", all_proxy="http://127.0.0.1:9", no_proxy="")
        self.process = subprocess.Popen([str(binary)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        text=True, env=environment)
        self.lines = queue.Queue()
        self.reader = threading.Thread(target=lambda: [self.lines.put(line) for line in self.process.stdout], daemon=True)
        self.reader.start()

    def request(self, **command):
        self.process.stdin.write(json.dumps(command) + "\n")
        self.process.stdin.flush()
        return json.loads(self.lines.get(timeout=25))

    def close(self):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=5)
        self.process.stdout.close()


def expect(response, body, status=200):
    if response.get("ok") is not True or response.get("status") != status or response.get("body") != body:
        raise AssertionError("unexpected_fixture_response:" + json.dumps(response))


def exercise(binary, report):
    require_disposable_runner()
    result = {"platform": platform.platform(), "coreVersion": "0.1.12", "source": os.environ.get("GITHUB_SHA"),
              "binarySha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "cases": [], "cleanup": False}
    native, probe, servers, aliases = None, None, [], []
    def record(name, response):
        result["cases"].append({"name": name, "passed": True, **response})
    try:
        native = MacProxy() if platform.system() == "Darwin" else WindowsProxy()
        alias(ADDRESS, True)
        aliases.append(ADDRESS)
        origin, other, first, second, pac = [Fixture(label) for label in ("direct", "other", "proxy-1", "proxy-2", "pac")]
        servers = [origin, other, first, second, pac]
        direct = f"http://{ADDRESS}:{origin.port}/message"
        alternate = f"http://{ADDRESS}:{other.port}/attachment"
        proxy_only = "http://awiki-native-fixture.invalid/message"
        native.set()
        probe = Probe(binary)
        response = probe.request(policy="system", url=direct)
        expect(response, "direct")
        record("no-system-proxy", response)
        native.set(first.port)
        response = probe.request(url=proxy_only)
        expect(response, "proxy-1")
        record("native-static-proxy", response)
        before = response["diagnostics"]["generation"]
        native.set(second.port)
        response = probe.request(url=proxy_only)
        expect(response, "proxy-2")
        assert response["diagnostics"]["generation"] > before
        record("native-proxy-change-invalidates-cache", response)
        native.set(second.port, bypass=ADDRESS)
        response = probe.request(url=direct)
        expect(response, "direct")
        record("native-bypass", response)
        pac.pac = 'function FindProxyForURL(url,host) { return host === "awiki-native-fixture.invalid" ? "PROXY 127.0.0.1:%d" : "DIRECT"; }' % first.port
        native.set(pac=f"http://127.0.0.1:{pac.port}/pac.js")
        response = probe.request(url=proxy_only)
        expect(response, "proxy-1")
        assert pac.events
        record("native-pac-proxy", response)
        response = probe.request(url=alternate)
        expect(response, "other")
        record("native-pac-direct", response)
        native.set(first.port)
        first.mode = "hang"
        response = probe.request(url=direct)
        expect(response, "direct")
        assert response["elapsedMs"] < 10000
        assert response["diagnostics"]["status"] == "ProxyFallback"
        record("proxy-stalls-direct-fallback", response)
        attempts = len(first.events)
        response = probe.request(url=alternate)
        expect(response, "other")
        assert len(first.events) == attempts
        record("failed-proxy-cooldown-across-origins", response)
        native.set(9)
        response = probe.request(url=direct)
        expect(response, "direct")
        record("closed-proxy-direct-fallback", response)
        second.mode = "auth"
        native.set(second.port)
        response = probe.request(url=direct)
        expect(response, "direct")
        record("proxy-auth-rejection-direct-fallback", response)
        second.mode = "deny"
        # New native settings invalidate the cooled-down proxy.
        native.set(second.port, bypass="unrelated.invalid")
        response = probe.request(url=proxy_only)
        expect(response, "proxy-2", 403)
        record("business-403-preserved", response)
        response = probe.request(url=direct)
        expect(response, "proxy-2", 403)
        second.mode = "drop-write"
        before_proxy, before_origin = len(second.events), len(origin.events)
        response = probe.request(url=direct, method="POST")
        assert response.get("ok") is False
        assert sum(item["method"] == "POST" for item in second.events[before_proxy:]) == 1
        assert len(origin.events) == before_origin
        record("submitted-write-not-replayed", response)
        response = probe.request(policy="direct", url=direct)
        expect(response, "direct")
        record("direct-disables-system-and-environment-proxy", response)
        native.set()
        response = probe.request(policy="system", url=direct)
        before = response["diagnostics"]["generation"]
        alias(CHANGE_ADDRESS, True)
        aliases.append(CHANGE_ADDRESS)
        time.sleep(1)
        observed = probe.request()["diagnostics"]["generation"] > before
        response = probe.request(url=direct)
        expect(response, "direct")
        assert response["diagnostics"]["generation"] > before
        record("native-interface-change-invalidates-cache", {**response, "observedBeforeNextRequest": observed})
        assert all(not event["business"] for server in servers for event in server.events if event["method"] == "CONNECT")
        record("connection-probes-have-no-business-headers", {})
        result["passed"] = True
    except Exception as exc:
        result["passed"] = False
        result["error"] = str(exc) or type(exc).__name__
        raise
    finally:
        errors = []
        for cleanup in ([probe.close] if probe else []) + ([native.restore] if native else []) + [lambda a=a: alias(a, False) for a in reversed(aliases)] + [server.close for server in servers]:
            try:
                cleanup()
            except Exception as exc:
                errors.append(type(exc).__name__)
        result["cleanup"] = not errors
        result["cleanupErrors"] = errors
        report.parent.mkdir(parents=True, exist_ok=True)
        report.write_text(json.dumps(result, indent=2) + "\n")
        if errors:
            raise RuntimeError("native_fixture_cleanup_failed")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    exercise(args.binary.resolve(), args.report)
