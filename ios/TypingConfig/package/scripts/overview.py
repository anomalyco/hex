"""What is in the export: days, phone, Hex builds, settings, layouts, sessions, words, and
what the phone had already learned (state/phone.json). Run this first.

    python3 scripts/overview.py [EXPORT_DIR]
"""
import json, os, sys, collections
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import hexlog, words as W

ex = hexlog.export_dir(sys.argv[1:])
files = hexlog.files_in(ex)
c = collections.Counter(); builds = collections.Counter(); settings = {}; phones = collections.Counter()
layouts = collections.Counter(); kinds = collections.Counter(); dur = 0.0; taps = 0
for f in files:
    for line in open(f, encoding="utf-8"):
        e = json.loads(line)
        c[e["type"]] += 1
        if e["type"] == "session_start":
            builds[(e.get("version"), e.get("build"), e.get("surface"))] += 1
            phones[(e.get("device"), e.get("system"), e.get("orientation"))] += 1
            settings[(e.get("version"), e.get("build"))] = e.get("settings")
            kinds[(e.get("traits") or {}).get("keyboardType")] += 1
        elif e["type"] == "layout" and "keys" in e:
            layouts[(e.get("width"), e.get("height"), e.get("language"), e.get("letters"), e.get("layer"), e.get("hexPlacement"))] += 1
        elif e["type"] == "session_end":
            dur += (e.get("durationMS") or 0) / 1000
        elif e["type"] == "touch" and e.get("phase") == "began":
            taps += 1
print("export:", ex)
print("log files:", ", ".join(os.path.basename(f) for f in files))
print("events:", dict(c.most_common()))
print("phone, system, orientation (sessions):", dict(phones))
print("Hex version, build, surface (sessions):", {"%s (%s) %s" % k: n for k, n in builds.items()})
for k, s in settings.items():
    print("settings on %s (%s):" % k, json.dumps(s, sort_keys=True))
print("field keyboard types (sessions):", dict(kinds))
print("distinct key layouts (width, height, language, letters, layer, Hex key):")
for k, n in layouts.most_common():
    print("   ", k, n)
print("keyboard on screen: %.1f min; touches: %d" % (dur / 60, taps))
words, LAY, sessions = W.load(ex)
by = collections.Counter(cl for w in words for cl in w["cls"])
sc = [w for w in words if W.scorable(w)]
print("words: %d; scorable (meaning known, Hex decided): %d; halves A/B: %d/%d" % (
    len(words), len(sc), sum(w["half"] == "A" for w in sc), sum(w["half"] == "B" for w in sc)))
print("word classes:", dict(by.most_common()))
sp = os.path.join(ex, "state", "phone.json")
if os.path.exists(sp):
    st = json.load(open(sp))
    print("\nstate/phone.json (what Hex had learned when the export was made):")
    for k, v in st.items():
        s = json.dumps(v, ensure_ascii=False)
        print("  %s: %s" % (k, s if len(s) < 300 else s[:300] + "…"))
else:
    print("\nno state/phone.json: you can't tell what Hex already learned; say so in the report")
