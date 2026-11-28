"""What an aim would do, as near as a script can tell without the phone: for every tap meant
for a letter, which letter Spatial autocorrect's touch model finds likeliest (a Gaussian
around each centre, Hex's spread: 0.29 of the key pitch across, 0.26 down) with Hex's
centres now (its learned averages, state/phone.json tapOffsets) and with the aim replacing
them for the letters it names. Gained: the meant letter becomes the likeliest; lost: it stops
being. The language model also weighs in on the phone, so this is the touch model's half only.

    python3 scripts/simulate_aim.py [EXPORT_DIR] --aim o:0,0.28 p:0,0.25 [--half B]
    python3 scripts/simulate_aim.py [EXPORT_DIR] --config typing-config.mhexconfig [--half B]
"""
import json, math, os, sys, collections
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import hexlog, taps as T, geometry as G
from limits import LIMITS


def centres(layout, offsets):
    out = {}
    for key in G.letter_ids(layout):
        f = G.keycap(layout, key); o = offsets.get(key, (0, 0))
        out[key] = (f[0] + f[2] / 2 + o[0] * f[2], f[1] + f[3] / 2 + o[1] * f[3])
    return out


def likeliest(c, sx, sy, x, y):
    return min(c, key=lambda k: ((x - c[k][0]) / sx) ** 2 + ((y - c[k][1]) / sy) ** 2)


def simulate(export, aim, half):
    """Per aim letter: {"taps", "gained", "lost"} on `half` (None: every tap)."""
    layout = G.load_layout(export)
    phone = json.load(open(os.path.join(export, "state", "phone.json")))
    learned = {k: tuple(v) for k, v in ((phone.get("tapOffsets") or {}).get("keycaps") or {}).items()}
    now = centres(layout, learned)
    new = centres(layout, dict(learned, **{k: tuple(v) for k, v in aim.items()}))
    A = LIMITS["keys"]["aim"]
    sx = A["sigmaX"] * (layout["letterWidth"] + layout["gap"]); sy = A["sigmaY"] * (layout["keyHeight"] + layout["rowGap"])
    tally = {k: {"taps": 0, "gained": 0, "lost": 0} for k in aim}
    for t in T.load(export, half):
        m = t["meant"]
        if m not in now:
            continue
        if m in tally:
            tally[m]["taps"] += 1
        a, b = likeliest(now, sx, sy, t["x"], t["y"]), likeliest(new, sx, sy, t["x"], t["y"])
        if a == b:
            continue
        if b == m and a != m:
            owner = m if m in aim else (a if a in aim else b)
            if owner in tally: tally[owner]["gained"] += 1
        elif a == m and b != m:
            owner = m if m in aim else (b if b in aim else a)
            if owner in tally: tally[owner]["lost"] += 1
    return tally


def aim_from_args(argv, export):
    if "--config" in argv:
        cfg = json.load(open(argv[argv.index("--config") + 1]))
        ctx = G.load_layout(export)["context"]
        for block in cfg.get("keys") or []:
            if block.get("context") == ctx:
                return {k: v["offset"] for k, v in (block.get("aim") or {}).items()}
        return {}
    out = {}
    if "--aim" in argv:
        for a in argv[argv.index("--aim") + 1:]:
            if a.startswith("--"): break
            k, v = a.split(":"); out[k] = [float(x) for x in v.split(",")]
    return out


if __name__ == "__main__":
    ex = hexlog.export_dir([a for a in sys.argv[1:] if os.path.isdir(a)])
    half = sys.argv[sys.argv.index("--half") + 1] if "--half" in sys.argv else None
    aim = aim_from_args(sys.argv, ex)
    if not aim:
        raise SystemExit("give --aim letter:dx,dy ... or --config FILE")
    tally = simulate(ex, aim, half)
    print("aim on half %s (touch model only):" % (half or "A+B"))
    for k in sorted(tally):
        v = tally[k]
        print("  %s %s: meant taps %d, gained %d, lost %d" % (k, aim[k], v["taps"], v["gained"], v["lost"]))
    print("  total: gained %d, lost %d" % (sum(v["gained"] for v in tally.values()), sum(v["lost"] for v in tally.values())))
    print("evidence json:", json.dumps({k: {"taps": v["taps"], "gained": v["gained"], "lost": v["lost"]} for k, v in tally.items()}))
