"""One isolated launch; invoked only after the standalone probe is negative."""
import json
import os
from pathlib import Path
import signal
import subprocess

root = Path.home() / "hex-click-probe"
data = root / "hex-test"
data.mkdir(mode=0o700, exist_ok=False)
(data / "settings.json").write_text(json.dumps({
    "sound_effects": False,
    "sound_effect_volume": 0.0,
}))
binary = root / "Hex Test (ours).app/Contents/MacOS/hex"
assert b"HEX_APPLICATION_SUPPORT_DIR" in binary.read_bytes(), "No isolated-data override"
env = os.environ.copy()
env.update({
    "HEX_APPLICATION_SUPPORT_DIR": str(data),
    "DYLD_INSERT_LIBRARIES": str(root / "click-probe.dylib"),
})
env.pop("CLICK_PROBE_UNHIDE", None)
with (root / "hex-onboarding.log").open("wb") as output:
    child = subprocess.Popen([str(binary)], env=env, stdout=output,
                             stderr=subprocess.STDOUT, start_new_session=True)
    print(f"Started isolated test pid={child.pid}", flush=True)
    try:
        result = child.wait(timeout=70)
    except subprocess.TimeoutExpired:
        os.killpg(child.pid, signal.SIGKILL)
        result = child.wait()
        print("Outer watchdog killed only the isolated test process group", flush=True)
print(f"Isolated test exit={result}", flush=True)
log = (root / "hex-onboarding.log").read_text(errors="replace")
print("\n".join(line for line in log.splitlines() if line.startswith("CLICK_PROBE ")))
assert "CLICK_PROBE loaded" in log, "Native window instrumentation did not load"
assert "CLICK_PROBE PASS:" in log or "CLICK_PROBE FAIL:" in log, "Incomplete sample"
