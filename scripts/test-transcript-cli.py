#!/usr/bin/env python3
"""Release CLI smoke with an occupied app lock and unchanged fixture files."""

import fcntl
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

binary = str(Path(sys.argv[1]).resolve())
repo = Path(__file__).resolve().parent.parent

with tempfile.TemporaryDirectory(prefix="hex-transcript-cli-") as directory:
    root = Path(directory)
    shutil.copy(repo / "tests/fixtures/history.json", root / "history.json")
    shutil.copytree(repo / "tests/fixtures/meetings", root / "meetings")
    (root / "settings.json").write_text('{"history_retention":"forever"}')
    lock = (root / "listener.lock").open("w")
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)

    def snapshot():
        return {
            str(path.relative_to(root)): (path.read_bytes(), path.stat().st_mtime_ns)
            for path in root.rglob("*") if path.is_file()
        }

    before = snapshot()
    env = {**os.environ, "HEX_APPLICATION_SUPPORT_DIR": str(root), "HEX_EXPERIMENTAL_MIC": "1"}

    def run(*args):
        result = subprocess.run([binary, *args], env=env, capture_output=True, text=True, timeout=10)
        assert result.returncode == 0, (args, result.stderr)
        assert not result.stderr, (args, result.stderr)
        return result.stdout

    history = json.loads(run("history", "--app", "OpenCode Beta", "--grep", "problems", "--json"))
    assert [entry["id"] for entry in history["entries"]] == [2, 1]
    assert history["version"] == 1
    assert all("raw_text" not in entry for entry in history["entries"])
    listing = json.loads(run("meetings", "list", "--json"))
    assert len(listing["meetings"]) == 3 and len(listing["skipped"]) == 1
    latest = json.loads(run("meetings", "show", "latest", "--json"))
    assert latest["meeting"]["id"] == "1790001000000-1003"
    assert latest["meeting"]["status"] == "interrupted"
    assert run("meetings", "show", "1790000500000-1002", "--md").startswith("# Roadmap review")
    assert "Roadmap review" in run("meetings", "search", "roadmap")
    assert snapshot() == before
    assert not (root / "logs").exists()
    assert not (root / "local-api.json").exists()
    print("Release transcript CLI: filters, JSON, meetings, occupied listener lock, no file writes passed")
