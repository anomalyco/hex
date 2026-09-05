#!/usr/bin/env python3
"""Display-free service/IPC regression. Never uses personal settings or a microphone."""
import json
import os
from pathlib import Path
import socket
import stat
import struct
import subprocess
import sys
import tempfile
import time

binary = str(Path(sys.argv[1]).resolve())


def wait_for(check, timeout=8):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = check()
        if result:
            return result
        time.sleep(0.025)
    raise AssertionError("timed out waiting for the service")


with tempfile.TemporaryDirectory(prefix="hex-service-", dir=os.environ.get("TMPDIR")) as directory:
    root = Path(directory)
    support = root / "data"
    alsa = root / "alsa.conf"
    alsa.write_text("# No physical audio devices in this test.\n")
    env = os.environ | {"HEX_APPLICATION_SUPPORT_DIR": str(support), "ALSA_CONFIG_PATH": str(alsa)}
    env.pop("DISPLAY", None)
    env.pop("WAYLAND_DISPLAY", None)
    path = support / "service.sock"
    children = []

    def launch():
        child = subprocess.Popen([binary, "service"], env=env, stdin=subprocess.DEVNULL,
                                 stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        children.append(child)
        return child

    def connection():
        client = socket.socket(socket.AF_UNIX)
        client.settimeout(4)
        client.connect(str(path))
        return client

    def exact(client, size):
        data = b""
        while len(data) < size:
            chunk = client.recv(size - len(data))
            assert chunk, "service disconnected"
            data += chunk
        return data

    def call(client, request="Snapshot", version=1):
        body = json.dumps({"version": version, "request": request}).encode()
        client.sendall(struct.pack(">I", len(body)) + body)
        size, = struct.unpack(">I", exact(client, 4))
        return json.loads(exact(client, size))

    def snapshot():
        try:
            with connection() as client:
                return call(client)["state"]
        except (OSError, AssertionError):
            return None

    try:
        service = launch()
        state = wait_for(snapshot)
        assert state["pid"] == service.pid
        assert state["desktop"]["listener"]["status"] == "Model required"
        assert stat.S_IMODE(path.stat().st_mode) == 0o600
        assert stat.S_IMODE(support.stat().st_mode) == 0o700

        duplicate = launch()
        assert duplicate.wait(timeout=5) != 0
        assert snapshot()["pid"] == service.pid

        # A Settings client can leave and reconnect without replacing the service.
        with connection() as client:
            assert call(client)["state"]["pid"] == service.pid
            assert call(client, {"SetVolume": 0.0})["error"] is None
        assert snapshot()["pid"] == service.pid
        assert json.loads((support / "linux-settings.json").read_text())["sound_effect_volume"] == 0

        # Disconnecting an uncommitted focused shortcut capture releases only that edit.
        with connection() as client:
            assert call(client, "CaptureShortcut")["state"]["capturing"]
            with connection() as other:
                assert call(other, "CancelShortcut")["error"]
        wait_for(lambda: (s := snapshot()) and not s["editing"])
        assert service.poll() is None

        with connection() as client:
            assert call(client, version=999)["error"]
        with connection() as client:
            client.sendall(struct.pack(">I", 128 * 1024 + 1))
            assert client.recv(1) == b""
        # A stalled request cannot stall other clients or live runtime state.
        with connection() as stalled:
            stalled.sendall(b"\x00")
            assert snapshot()["pid"] == service.pid
            assert stalled.recv(1) == b""

        status = subprocess.run([binary, "status"], env=env, check=True, capture_output=True, text=True)
        assert json.loads(status.stdout)["pid"] == service.pid

        # Crash leaves a socket, not a permanent instance lock. No stale state is reported.
        service.kill()
        service.wait(timeout=5)
        assert subprocess.run([binary, "status"], env=env, capture_output=True).returncode != 0
        service = launch()
        state = wait_for(snapshot)
        assert state["pid"] == service.pid
        assert state["settings"]["sound_effect_volume"] == 0
        subprocess.run([binary, "stop"], env=env, check=True, capture_output=True)
        assert service.wait(timeout=8) == 0
        assert not path.exists()
        assert (support / "linux-settings.json").exists()
        # Custom data roots must never start/restart the user's real installed unit.
        fakebin = root / "bin"
        fakebin.mkdir()
        marker = root / "unexpected-systemctl"
        fake = fakebin / "systemctl"
        fake.write_text(f'#!/bin/sh\ntouch "{marker}"\nexit 0\n')
        fake.chmod(0o755)
        isolated_env = env | {"DISPLAY": ":invalid", "PATH": f'{fakebin}:{env["PATH"]}'}
        for command in ("start", "restart"):
            assert subprocess.run([binary, command], env=isolated_env, capture_output=True).returncode != 0
        assert not marker.exists()
    finally:
        for child in children:
            if child.poll() is None:
                child.kill()
            child.wait(timeout=5)

print("Linux service IPC/lifecycle checks passed.")
