"""Where the fingers land for each key: the cloud of taps **meant** for it, against its keycap
on the keyboard the person uses now. Per key: taps on each half, the mean offset in keycaps
(dx right, dy down), its spread, how many pressed another key, the mean of the taps that
pressed it (what Learn how I type averages), what Hex had learned (state/phone.json), and the
aim the config would get from the tuning half (shrunk by n/(n+20), clamped to +-0.3) with the
check half's own mean beside it.

    python3 scripts/key_clouds.py [EXPORT_DIR] [--tune A|B]
"""
import json, os, sys, statistics as st, collections
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import hexlog, taps as T, geometry as G
from limits import LIMITS

ex = hexlog.export_dir(sys.argv[1:])
tune = sys.argv[sys.argv.index("--tune") + 1] if "--tune" in sys.argv else "A"
check = "B" if tune == "A" else "A"
layout = G.load_layout(ex)
A = LIMITS["keys"]["aim"]
taps = [t for t in T.load(ex) if t["meant"] != "space"]
phone = json.load(open(os.path.join(ex, "state", "phone.json")))
learned = (phone.get("tapOffsets") or {}).get("keycaps") or {}


def rel(t, key):
    f = G.keycap(layout, key)
    return (t["x"] - f[0] - f[2] / 2) / f[2], (t["y"] - f[1] - f[3] / 2) / f[3]


def mean(v):
    return (st.mean(a for a, _ in v), st.mean(b for _, b in v)) if v else (0.0, 0.0)


print("aim tuned on half %s, checked on half %s; minimums: %d tuned and %d checked taps" % (tune, check, A["minTunedTaps"], A["minCheckedTaps"]))
print("%-4s %5s %5s %6s %15s %11s %15s %15s %15s %15s %s" % ("key", tune, check, "miss", "meant mean", "spread", "pressed mean", "Hex learned", "aim (tuned)", "checked mean", "candidate"))
candidates = {}
for key in G.letter_ids(layout):
    mine = [t for t in taps if t["meant"] == key]
    if not mine:
        continue
    tv = [rel(t, key) for t in mine if t["half"] == tune]
    cv = [rel(t, key) for t in mine if t["half"] == check]
    pressed = [rel(t, key) for t in T.load(ex) if t["now"] == key and t["meant"] != "space"]
    allv = tv + cv
    miss = sum(1 for t in mine if t["now"] != key) / len(mine)
    m, pm, tm, cm = mean(allv), mean(pressed), mean(tv), mean(cv)
    spread = (st.pstdev(a for a, _ in allv), st.pstdev(b for _, b in allv)) if len(allv) > 1 else (0, 0)
    n = len(tv)
    shrink = n / (n + A["shrinkPrior"]) if n else 0
    aim = [max(-A["bound"], min(A["bound"], round(tm[0] * shrink, 3))), max(-A["bound"], min(A["bound"], round(tm[1] * shrink, 3)))]
    hex_ = learned.get(key)
    drift = max(abs(cm[0] - aim[0]), abs(cm[1] - aim[1])) if cv else 9
    change = max(abs(aim[0] - (hex_[0] if hex_ else 0)), abs(aim[1] - (hex_[1] if hex_ else 0)))
    ok = len(tv) >= A["minTunedTaps"] and len(cv) >= A["minCheckedTaps"] and drift <= A["maxCheckDrift"] and change >= A["minChange"]
    why = "yes (simulate_aim.py next)" if ok else ("too few taps" if len(tv) < A["minTunedTaps"] or len(cv) < A["minCheckedTaps"] else "check half differs" if drift > A["maxCheckDrift"] else "same as Hex's")
    print("%-4s %5d %5d %5.1f%% %+6.2f,%+6.2f %4.2f,%4.2f %+6.2f,%+6.2f %15s %+6.2f,%+6.2f %+6.2f,%+6.2f %s" % (
        key, len(tv), len(cv), 100 * miss, m[0], m[1], spread[0], spread[1], pm[0], pm[1],
        ("%+6.2f,%+6.2f" % tuple(hex_)) if hex_ else "-", aim[0], aim[1], cm[0], cm[1], why))
    if ok:
        candidates[key] = aim
print("\ncandidate aim (keycaps), to check with simulate_aim.py --aim:")
print(" ".join("%s:%g,%g" % (k, v[0], v[1]) for k, v in sorted(candidates.items())) or "(none)")
rows = collections.defaultdict(list)
for t in taps:
    k = next((k for k in layout["keys"] if k["id"] == t["meant"]), None)
    if k: rows[k["row"]].append(rel(t, t["meant"]))
print("\nby row (all halves): " + "; ".join("row %d: n %d, mean %+.2f,%+.2f" % (r, len(v), *mean(v)) for r, v in sorted(rows.items())))
