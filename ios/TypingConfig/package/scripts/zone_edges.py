"""Every edge a zone may move, with the taps that fell just across it: taps meant for the key
that pressed the neighbour beyond that edge, and taps meant for a neighbour that landed on
the key's side, within a quarter keycap of the edge. Edges with many of the first and few of
the second are worth a sweep with simulate_zones.py. The space bar and the bottom letter row
come first.

    python3 scripts/zone_edges.py [EXPORT_DIR] [--half A]
"""
import os, sys, collections
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import hexlog, taps as T, geometry as G
from limits import LIMITS

ex = hexlog.export_dir([a for a in sys.argv[1:] if os.path.isdir(a)])
half = sys.argv[sys.argv.index("--half") + 1] if "--half" in sys.argv else None
layout = G.load_layout(ex)
base = G.base_regions(layout)
taps = T.load(ex, half)
band_v, band_h = 0.25 * layout["keyHeight"], 0.25 * layout["letterWidth"]
rows = []
for i, k in enumerate(layout["keys"]):
    if k["row"] < 0:
        continue
    edges = G._edges_for(k["id"]) or []
    rects = base[i]
    minx = min(r[0] for r in rects); miny = min(r[1] for r in rects)
    maxx = max(r[0] + r[2] for r in rects); maxy = max(r[1] + r[3] for r in rects)
    for edge in edges:
        line = {"top": miny, "bottom": maxy, "left": minx, "right": maxx}[edge]
        ta = layout["touchArea"]
        if (edge == "top" and line <= ta[1] + 0.01) or (edge == "bottom" and line >= ta[1] + ta[3] - 0.01) or \
           (edge == "left" and line <= ta[0] + 0.01) or (edge == "right" and line >= ta[0] + ta[2] - 0.01):
            continue
        across = mine = risk = 0
        neighbours = collections.Counter()
        for t in taps:
            x, y = t["x"], t["y"]
            if edge in ("top", "bottom"):
                if not (minx <= x < maxx): continue
                d = (line - y) if edge == "top" else (y - line)
                band = band_v
            else:
                if not (miny <= y < maxy): continue
                d = (line - x) if edge == "left" else (x - line)
                band = band_h
            if 0 <= d < band and t["meant"] == k["id"] and t["now"] != k["id"]:
                across += 1; neighbours[t["now"]] += 1
            elif 0 <= d < band and t["meant"] != k["id"] and t["now"] == t["meant"]:
                risk += 1
            elif -band < d < 0 and t["meant"] != k["id"] and t["now"] == k["id"]:
                mine += 1
        if across or mine:
            rows.append((k["id"], edge, across, risk, mine, neighbours))
rows.sort(key=lambda r: (-(r[0] == "space" or (len(r[0]) == 1 and r[0] in "zxcvbnm")), -(r[2] - r[3])))
print("half %s: taps within a quarter keycap of each edge (sweep the top ones with simulate_zones.py --sweep key:edge)" % (half or "A+B"))
print("  fell out: meant for the key, landed just beyond this edge on a neighbour (growing the key gains them)")
print("  at risk:  the neighbour's own taps in that band (growing the key would lose them)")
print("  came in:  a neighbour's taps that landed just inside this edge (shrinking the key gains them)")
print("%-14s %-7s %8s %8s %8s  %s" % ("key", "edge", "fell out", "at risk", "came in", "pressed instead"))
for key, edge, across, risk, mine, nb in rows[:30]:
    print("%-14s %-7s %8d %8d %8d  %s" % (key, edge, across, risk, mine, ", ".join("%s %d" % x for x in nb.most_common(3))))
