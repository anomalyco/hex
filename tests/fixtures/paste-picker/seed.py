"""Build synthetic files for a disposable Mac receiver run. Never start Hex or
read a user's folders. Copy the result to /Users/admin/lab in the guest."""

import json
import os
from pathlib import Path
import struct
import sys
import time
import wave
import zlib

root = Path(sys.argv[1])
root.mkdir(exist_ok=False)
now = int(time.time() * 1000)
words = "Keep the dates on one line."
note = "Move the caption below the chart."
bundle = root / "captures" / "ready.scrawl"
bundle.mkdir(parents=True)


def save(path, data):
    path.write_text(json.dumps(data))


save(root / "settings.json", {"capture": {"enabled": True, "folder": "/Users/admin/lab/captures"}})
(root / "onboarding-complete").touch()
save(root / "history.json", {"version": 1, "next_id": 2, "entries": [
    {"id": 1, "timestamp_ms": now-60000, "kind": "dictation", "raw_text": words, "final_text": words}]})
save(bundle / "scrawl.json", {"version": 1, "id": "125000", "createdAtMs": now, "durationMs": 4000,
     "target": {"mode": "display", "displayId": 1, "frame": {"x": 0, "y": 0, "width": 240, "height": 160, "scale": 1}},
     "transcript": {"source": "hex", "text": note, "segments": [], "words": []}, "strokes": [], "marks": [], "events": [],
     "keyframes": [{"index": 1, "t": 0, "reason": "initial", "file": "frames/frame-01.png", "event": "Chart", "said": note}],
     "contactSheet": "contact-sheet.png"})
save(bundle / ".hex-annotate-v1", {"generation": list(os.urandom(16)), "ready_at": now})


def chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))


(bundle / "contact-sheet.png").write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 32, 32, 8, 2, 0, 0, 0)) +
    chunk(b"IDAT", zlib.compress((b"\0" + b"\xb3\xc2\xd1"*32)*32)) + chunk(b"IEND", b""))
with wave.open(str(root / "silence.wav"), "wb") as f:
    f.setparams((1, 2, 16000, 0, "NONE", "none"))
    f.writeframes(bytes(32000))
