#!/usr/bin/env python3
"""Replay synthetic HEVC captures without a screen, microphone, or live profile.

Pass capture-core's cargo-nextest JSON listing, optionally pool mode (0/1)
and an output label. Mode 0 is the regression; mode 1 is a diagnostic control.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

listing = json.loads(Path(sys.argv[1]).read_text())
test = "video_writer::tests::headless_encoder_memory"
binary = next(v["binary-path"] for v in listing["rust-suites"].values()
              if test in v["testcases"])
pooled = sys.argv[2] if len(sys.argv) > 2 else "0"
assert pooled in ("0", "1")
label = sys.argv[3] if len(sys.argv) > 3 else pooled
out = (Path(".lab-output/headless-pipelines/encoder") / label).resolve()
out.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory(prefix="hex-encoder-memory-") as home:
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(("HEX_", "OPENCODE_", "RUNE_", "OTEL_"))}
    env.update(HOME=home, HEX_APPLICATION_SUPPORT_DIR=home + "/data",
               HEX_DISABLE_UPDATES="1", HEX_MEMORY_OUT=str(out), HEX_MEMORY_POOL=pooled)
    with (out / "test.log").open("w") as log:
        result = subprocess.run([binary, "--ignored", "--exact", test, "--nocapture"],
                                env=env, stdout=log, stderr=log, timeout=600)
    print((out / "test.log").read_text())
    if result.returncode:
        sys.exit(result.returncode)


def size(text):
    return float(text[:-1]) * {"K": 1024, "M": 1024**2, "G": 1024**3}[text[-1]]


def resident_surfaces(cycle):
    text = (out / f"encoder-{cycle:03}-vmmap.txt").read_text()
    row = next(line for line in text.splitlines() if line.startswith("IOSurface "))
    return size(row.split()[2])


# Synthetic frames are 640x360. Allow driver warm-up, but catch the observed
# 0.7 MiB/capture accumulation well before another 51 captures can exhaust it.
growth = resident_surfaces(50) - resident_surfaces(0)
print(json.dumps({"iosurface_resident_growth_bytes": growth, "cycles": 51}))
assert growth < 4 * 1024**2, f"encoder retained {growth} bytes of IOSurfaces"
