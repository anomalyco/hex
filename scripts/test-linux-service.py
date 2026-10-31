#!/usr/bin/env python3
"""Display-free single-instance regression. Never uses personal settings or a microphone.

One Hex process owns a data root. A second launch forwards to it and exits;
`hex` commands forward over the same socket and fail fast without it.
"""
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
    raise AssertionError("timed out waiting for Hex")


with tempfile.TemporaryDirectory(prefix="hex-instance-", dir=os.environ.get("TMPDIR")) as directory:
    root = Path(directory)
    support = root / "data"
    support.mkdir(mode=0o700)
    # Never listen: no microphone, no shortcuts portal registration.
    (support / "settings.json").write_text(json.dumps({"enable_on_launch": False}))
    alsa = root / "alsa.conf"
    alsa.write_text("# No physical audio devices in this test.\n")
    # Custom data roots must never start, stop, or restart the user's real unit.
    fakebin = root / "bin"
    fakebin.mkdir()
    marker = root / "unexpected-systemctl"
    fake = fakebin / "systemctl"
    fake.write_text(f'#!/bin/sh\ntouch "{marker}"\necho loaded\nexit 0\n')
    fake.chmod(0o755)
    env = os.environ | {
        "HEX_APPLICATION_SUPPORT_DIR": str(support),
        # Hex tidies its launcher and autostart entries at startup.
        "XDG_DATA_HOME": str(root / "xdg-data"),
        "XDG_CONFIG_HOME": str(root / "xdg-config"),
        "ALSA_CONFIG_PATH": str(alsa),
        "PATH": f'{fakebin}:{os.environ["PATH"]}',
    }
    # No display: a window request fails the window, never Hex.
    for key in ("DISPLAY", "WAYLAND_DISPLAY", "HEX_SYSTEMD_UNIT", "NOTIFY_SOCKET"):
        env.pop(key, None)
    path = support / "service.sock"
    children = []

    def spawn(*arguments):
        child = subprocess.Popen([binary, *arguments], env=env, stdin=subprocess.DEVNULL,
                                 stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        children.append(child)
        return child

    def run(*arguments, timeout=10):
        return subprocess.run([binary, *arguments], env=env, capture_output=True, text=True,
                              timeout=timeout)

    def connection():
        client = socket.socket(socket.AF_UNIX)
        client.settimeout(4)
        client.connect(str(path))
        return client

    def exact(client, size):
        data = b""
        while len(data) < size:
            chunk = client.recv(size - len(data))
            assert chunk, "Hex disconnected"
            data += chunk
        return data

    def call(client, request="Status", version=2):
        body = json.dumps({"version": version, "request": request}).encode()
        client.sendall(struct.pack(">I", len(body)) + body)
        size, = struct.unpack(">I", exact(client, 4))
        return json.loads(exact(client, size))

    def status():
        try:
            with connection() as client:
                return call(client)["status"]
        except (OSError, AssertionError):
            return None

    try:
        # Without an instance every forwarded command fails fast.
        for command in (["status"], ["dictation", "start"], ["paste-last"]):
            started = time.monotonic()
            missing = run(*command)
            assert missing.returncode != 0 and "not running" in missing.stderr, missing.stderr
            assert time.monotonic() - started < 2
        assert run("quit").returncode == 3, "quitting nothing says Hex isn't running"

        # `hex app --hidden` with no unit becomes the instance in this process.
        instance = spawn("app", "--hidden")
        state = wait_for(status)
        assert state["pid"] == instance.pid
        assert state["window_open"] is False
        assert state["desktop"]["listener"]["status"] == "Disabled"
        assert stat.S_IMODE(path.stat().st_mode) == 0o600
        assert stat.S_IMODE(support.stat().st_mode) == 0o700

        # A second launch forwards and exits 0, leaving the same instance;
        # `hex start` finds it running.
        for command in (["start"], ["app", "--hidden"], ["service"]):
            forwarded = run(*command)
            assert forwarded.returncode == 0, (command, forwarded.stderr)
            assert status()["pid"] == instance.pid

        # Activate reaches the window seam; without a display only the window fails.
        token = run("app")
        assert token.returncode == 0, token.stderr
        time.sleep(0.5)
        assert instance.poll() is None
        assert status()["pid"] == instance.pid

        with connection() as client:
            reply = call(client, {"Activate": {
                "activation": {"token": "test-token", "startup_id": None},
                "show": False,
                "session": state["session"],
            }})
            assert reply["error"] is None and reply["status"]["pid"] == instance.pid

        printed = run("status", "--json")
        assert printed.returncode == 0, printed.stderr
        printed = json.loads(printed.stdout)
        assert printed["status"] == "status" and printed["pid"] == instance.pid
        assert printed["listener"]["running"] is False
        # Control commands ride the same socket.
        assert run("settings", "set", "sound_effects", "off").returncode == 0
        assert run("settings", "get", "sound_effects").stdout.strip() == "false"
        refused = run("window", "story", "HUD")
        assert refused.returncode == 1 and "macOS-only" in refused.stderr, refused.stderr

        # Keybinding commands need a listening Hex; they fail fast without one,
        # with one line rather than an error report.
        refused = run("dictation", "toggle")
        assert refused.returncode != 0 and "not listening" in refused.stderr, refused.stderr
        assert refused.stderr.startswith("hex: ") and refused.stderr.count("\n") == 1, refused.stderr

        # Version 1 (service/Settings) clients get a version error, not a hang.
        with connection() as client:
            reply = call(client, "Snapshot", version=1)
            assert reply["version"] == 2 and reply["error"]
        with connection() as client:
            client.sendall(struct.pack(">I", 128 * 1024 + 1))
            assert client.recv(1) == b""
        # A stalled request cannot stall other clients.
        with connection() as stalled:
            stalled.sendall(b"\x00")
            assert status()["pid"] == instance.pid
            assert stalled.recv(1) == b""

        # A crash leaves a socket, not a permanent lock.
        instance.kill()
        instance.wait(timeout=5)
        assert path.exists()
        missing = run("paste-last")
        assert missing.returncode != 0 and "not running" in missing.stderr, missing.stderr
        assert missing.stderr.count("\n") == 1, missing.stderr

        # Two racing launches produce one instance; the other forwards and exits 0.
        first, second = spawn("app", "--hidden"), spawn("app", "--hidden")
        state = wait_for(status)
        owners = {first.pid, second.pid}
        assert state["pid"] in owners
        loser = second if state["pid"] == first.pid else first
        assert loser.wait(timeout=8) == 0
        instance = first if loser is second else second

        # Quit drains and exits 0, removing the socket and keeping settings.
        assert run("quit").returncode == 0
        assert instance.wait(timeout=8) == 0
        assert not path.exists()
        assert (support / "settings.json").exists()

        for command in (["quit"], ["dictation", "start"], ["paste-last"]):
            assert run(*command).returncode == 3, command
        assert not marker.exists(), "a custom data root ran systemctl"
    finally:
        for child in children:
            if child.poll() is None:
                child.kill()
            child.wait(timeout=5)

print("Linux single-instance checks passed.")
