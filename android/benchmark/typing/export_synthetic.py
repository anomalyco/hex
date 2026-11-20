"""Select only the iOS benchmark's public, generated English test items. Never export its logs or metadata."""
import json, re, sys
from pathlib import Path

source = Path(sys.argv[1])
items = json.loads((source / "input.json").read_text())["items"]
baselines = {key: json.loads((source / "results" / ("output-final-" + name + ".json")).read_text())["outputs"]
             for key, name in [("ios_previous", "today"), ("ios_spatial", "decoder"),
                                 ("ios_calibrated", "decoder-offsets")]}
selected = {r["id"]: r for r in items if r["set"] == "test" and re.fullmatch(r"test-[0-9]+", r["id"])
            and "logged" not in r and r["context"] == "" and r.get("language", "en") == "en"}
assert set(selected) == {"test-" + str(i) for i in range(2000)}
for i in range(2000):
    r = selected["test-" + str(i)]
    assert re.fullmatch(r"[a-z ]+", r["typed"]) and len(r["typed"]) == len(r["taps"])
    out = {f: r[f] for f in ("id", "intended", "typed", "taps")}
    out.update({k: v[r["id"]] for k, v in baselines.items()})
    print(json.dumps(out, separators=(",", ":")))
