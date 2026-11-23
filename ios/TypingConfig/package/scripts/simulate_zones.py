"""Exactly what proposed zones would do to the logged taps. Zones are geometry: given each
tap's landing point on today's keys and the key it was meant for (taps.py), this says which
key every tap would press with Hex's own edges and with the zones (compiled as the phone
compiles them, geometry.py). Gained: a tap that pressed another key now presses the one it
was meant for; lost: the reverse. Simulate a key block's zones together: they interact. The
next-letter odds and autocorrect act after this on the phone (the replay there checks those).

    python3 scripts/simulate_zones.py [EXPORT_DIR] space:top:+0.15 o:bottom:+0.1 [--half B]
    python3 scripts/simulate_zones.py [EXPORT_DIR] --config typing-config.mhexconfig [--half B]
    python3 scripts/simulate_zones.py [EXPORT_DIR] --sweep space:top [other zones held] [--half A]
"""
import json, os, sys, collections
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import hexlog, taps as T, geometry as G
from limits import LIMITS


def simulate(export, zones, half):
    """(per zone {"taps", "gained", "lost"} or {"skipped": reason}, totals, per key gained/lost)."""
    layout = G.load_layout(export)
    base = G.base_regions(layout)
    regions, skipped, strips = G.zone_map(layout, zones)
    per = []
    for n, z in enumerate(zones):
        per.append({"skipped": skipped[n]} if skipped[n] else {"taps": 0, "gained": 0, "lost": 0})
    total = {"taps": 0, "gained": 0, "lost": 0}
    keys = collections.Counter()
    for t in T.load(export, half):
        # Only a tap inside a zone's strips can press another key.
        owner = next((n for n in range(len(zones)) if not skipped[n] and any(G.contains(s[0], t["x"], t["y"]) for s in strips[n])), None)
        if owner is None:
            continue
        before = G.key_at(layout, base, t["x"], t["y"])
        after = G.key_at(layout, regions, t["x"], t["y"])
        per[owner]["taps"] += 1; total["taps"] += 1
        if before != t["meant"] and after == t["meant"]:
            per[owner]["gained"] += 1; total["gained"] += 1; keys[(t["meant"], "gained")] += 1
        elif before == t["meant"] and after != t["meant"]:
            per[owner]["lost"] += 1; total["lost"] += 1; keys[(t["meant"], "lost")] += 1
    return per, total, keys


def zones_from(argv, export):
    if "--config" in argv:
        cfg = json.load(open(argv[argv.index("--config") + 1]))
        ctx = G.load_layout(export)["context"]
        for block in cfg.get("keys") or []:
            if block.get("context") == ctx:
                return [{"key": z["key"], "edge": z["edge"], "move": z["move"]} for z in block.get("zones") or []]
        return []
    skip = {"--half", "--sweep", "--config"}
    out, i = [], 0
    args = [a for a in argv if not os.path.isdir(a)]
    while i < len(args):
        if args[i] in skip:
            i += 2; continue
        if args[i].count(":") == 2:
            out.append(G.parse_zone(args[i]))
        i += 1
    return out


def show(per, total, keys, zones):
    for z, p in zip(zones, per):
        name = "%s %s %+.2f" % (z["key"], z["edge"], z["move"])
        if "skipped" in p:
            print("  %-22s skipped: %s" % (name, p["skipped"]))
        else:
            print("  %-22s taps in its strip %3d, gained %3d, lost %3d, net %+d" % (name, p["taps"], p["gained"], p["lost"], p["gained"] - p["lost"]))
    print("  total: taps %d, gained %d, lost %d, net %+d" % (total["taps"], total["gained"], total["lost"], total["gained"] - total["lost"]))
    by = sorted({k for k, _ in keys})
    if by:
        print("  by meant key: " + ", ".join("%s +%d -%d" % (k, keys[(k, "gained")], keys[(k, "lost")]) for k in by))


if __name__ == "__main__":
    ex = hexlog.export_dir([a for a in sys.argv[1:] if os.path.isdir(a)])
    half = sys.argv[sys.argv.index("--half") + 1] if "--half" in sys.argv else None
    held = zones_from(sys.argv, ex)
    if "--sweep" in sys.argv:
        key, edge = sys.argv[sys.argv.index("--sweep") + 1].split(":")
        held = [z for z in held if not (z["key"] == key and z["edge"] == edge)]
        step, bound = LIMITS["keys"]["zones"]["step"], LIMITS["keys"]["zones"]["maxMove"]
        print("sweep %s %s on half %s (other zones held: %s)" % (key, edge, half or "A+B", held or "none"))
        print("  %6s %6s %6s %6s %6s  %s" % ("move", "taps", "gained", "lost", "net", "geometry"))
        n = int(round(bound / step))
        for i in [j for j in range(-n, n + 1) if j]:
            move = round(i * step, 2)
            per, total, _ = simulate(ex, held + [{"key": key, "edge": edge, "move": move}], half)
            p = per[-1]
            if "skipped" in p:
                print("  %+6.2f %26s  %s" % (move, "", p["skipped"]))
            else:
                print("  %+6.2f %6d %6d %6d %+6d" % (move, p["taps"], p["gained"], p["lost"], p["gained"] - p["lost"]))
        sys.exit(0)
    if not held:
        raise SystemExit("give zones as key:edge:move, --config FILE or --sweep key:edge")
    per, total, keys = simulate(ex, held, half)
    print("zones on half %s:" % (half or "A+B"))
    show(per, total, keys, held)
    print("evidence json:", json.dumps([p if "skipped" in p else {"taps": p["taps"], "gained": p["gained"], "lost": p["lost"]} for p in per]))
