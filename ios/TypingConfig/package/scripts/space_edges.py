"""The space bar and the edges around it: where space-bar touches land and whether each typed a
space; bottom-row letter taps that landed below their keycap (in the gap above the space bar);
words where a space was missed or typed in place of a letter; and touches on Return, 123 and
Delete near the letter rows. Counts and positions only.

    python3 scripts/space_edges.py [EXPORT_DIR] [--half A|B]
"""
import json, os, sys, collections, statistics as st
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import hexlog, words as W

ex = hexlog.export_dir(sys.argv[1:])
half = W.half_arg(sys.argv)
words, LAY, sessions = W.load(ex, half)
keep = {s for s in sessions if W.half_of(s) == half} if half else None
lay = {}
res = collections.Counter(); dys = []; dxs = []; low = []; edge = collections.Counter(); bars = {}
for f in hexlog.files_in(ex):
    events = [json.loads(l) for l in open(f, encoding="utf-8")]
    for e in events:
        if e["type"] == "layout" and "keys" in e:
            lay[e["hash"]] = {k["id"]: k for k in e["keys"]}
    for i, e in enumerate(events):
        if e["type"] != "touch" or e["phase"] != "began" or (keep is not None and e["session"] not in keep):
            continue
        L = lay.get(e["layout"]); k = e.get("key")
        if not L or not k or k not in L or "space" not in L:
            continue
        sp = L["space"]
        if k == "space":
            cap = sp["frame"]; hit = sp["hitFrame"]
            bars[(round(cap[1], 1), round(cap[3], 1), round(hit[1], 1))] = 1
            dy = e["y"] - (cap[1] + cap[3] / 2); dx = e["x"] - (cap[0] + cap[2] / 2)
            dys.append(dy); dxs.append(dx)
            zone = "above keycap" if e["y"] < cap[1] else "top quarter" if e["y"] < cap[1] + cap[3] / 4 else "rest"
            typed = "nothing"
            for e2 in events[i + 1:]:
                if e2.get("session") != e["session"]:
                    continue
                if e2["type"] == "touch" and e2["phase"] == "began" and e2.get("id") != e.get("id"):
                    break
                if e2["type"] == "ui" and e2.get("action") == "trackpad_start":
                    typed = "trackpad"; break
                if e2["type"] == "edit":
                    op = e2.get("op"); ins = e2.get("inserted") or ""
                    typed = "space" if ((op == "insert" and " " in ins) or (op == "autocorrect" and ins.endswith(" ")) or op in ("space_swallowed", "double_space_period", "suggestion")) else op
                    break
            res[(zone, typed)] += 1
        elif L[k].get("row") == 2 and len(k) == 1 and k.isalpha():
            cap = L[k]["frame"]
            if e["y"] > cap[1] + cap[3]:
                low.append(round(e["y"] - (cap[1] + cap[3]), 1))
        if k in ("return", "layer:numbers", "delete", "shift") and k in L:
            cap = L[k]["frame"]
            edge[(k, "outside keycap" if not (cap[0] <= e["x"] <= cap[0] + cap[2] and cap[1] <= e["y"] <= cap[1] + cap[3]) else "on keycap")] += 1
print("space bar geometry seen (keycap top, keycap height, hit-frame top):", list(bars))
n = len(dys)
if n:
    print("space-bar touches: %d; landing point vs keycap centre: dy mean %+.1f pt (sd %.1f), dx mean %+.1f pt (sd %.1f)" % (
        n, st.mean(dys), st.pstdev(dys), st.mean(dxs), st.pstdev(dxs)))
    q = sorted(dys)
    print("  dy percentiles 5/25/50/75/95: %s" % " ".join("%+.1f" % q[int(p * (n - 1))] for p in (0.05, 0.25, 0.5, 0.75, 0.95)))
    above = sum(c for (z, t), c in res.items() if z == "above keycap")
    top = sum(c for (z, t), c in res.items() if z == "top quarter")
    print("  above the keycap: %d (%.1f%%), in its top quarter: %d (%.1f%%)" % (above, 100 * above / n, top, 100 * top / n))
    print("  TYPICAL (Hex's reference; for now from one tester's logs): mean dy within +-3 pt of the centre, under 3%% above the keycap. This person: %s" % (
        "TYPICAL" if abs(st.mean(dys)) <= 3 and above / n < 0.03 else "NOT TYPICAL: report it as a finding even if no space was lost"))
print("space-bar touches by zone and what they did:")
for (z, t), c in sorted(res.items()):
    print("  %-13s %-22s %d" % (z, t, c))
print("bottom-row letter taps below their keycap (in the gap above the space bar): %d; depth below keycap (pt): %s" % (len(low), sorted(low)))
sc = [w for w in words if W.scorable(w)]
ms = collections.Counter(); sl = collections.Counter()
for w in words:
    for e in w.get("raw_errors") or []:
        if e[0] == "missed_space": ms["raw"] += 1
        if e[0] == "letter_for_space": ms["letter for space: " + e[1]] += 1
        if e[0] == "space_for_letter": sl[e[1]] += 1
split = sum(1 for w in words if w.get("action") == "autocorrect" and " " in W.norm(w.get("ac_to")))
print("words whose taps missed a space (a letter where a space was meant):", dict(ms), "; Hex split %d typed runs into two words" % split)
print("words where a space was tapped in place of a letter:", dict(sl))
print("touches on Return, 123, Delete, Shift, on or outside their keycap:", dict(edge))
