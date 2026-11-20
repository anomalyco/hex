"""Export only generated held-out Tatoeba rows and their same-model iOS outputs."""
import json, math, re, sys
from pathlib import Path

def export(source, result, lang):
    if lang not in ("en", "es"):
        raise ValueError("No synthetic set for this language")
    group = "test" if lang == "en" else "es-test"
    items = json.loads(Path(source).read_text())["items"]
    outputs = json.loads(Path(result).read_text())["outputs"]
    selected = [r for r in items if r["set"] == group and re.fullmatch(group + r"-[0-9]+", r["id"])
            and "logged" not in r and r["context"] == "" and r.get("language", "en") == lang]
    rows = {r["id"]: r for r in selected}
    if len(selected) != 2000 or set(rows) != {group + "-" + str(i) for i in range(2000)}:
        raise ValueError("Incomplete or duplicated synthetic set")
    for i in range(2000):
        r = rows[group + "-" + str(i)]
        allowed = r"[a-zñ ]+" if lang == "es" else r"[a-z ]+"
        if (not re.fullmatch(allowed, r["typed"]) or len(r["typed"]) != len(r["taps"]) or
                any(not isinstance(t, list) or len(t) != 2 or
                    not all(type(v) in (int, float) and math.isfinite(v) for v in t) for t in r["taps"])):
            raise ValueError("Invalid synthetic key geometry")
        out = {f: r[f] for f in ("id", "intended", "typed", "taps")}
        out["ios_shared"] = outputs[r["id"]]
        print(json.dumps(out, separators=(",", ":")))

if __name__ == "__main__":
    if len(sys.argv) != 4:
        sys.exit("export_synthetic.py INPUT.json R1-OUTPUT.json {en|es}")
    export(*sys.argv[1:])
