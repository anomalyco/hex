"""Exactly what proposed zones would do to the logged taps, and an estimate of what they would
do to the words. Zones are geometry: given each tap's landing point on today's keys and the key it
was meant for (taps.py), this says which key every tap would press with Hex's own edges and with
the zones (compiled as the phone compiles them, geometry.py). Gained: a tap that pressed another
key now presses the one it was meant for; lost: the reverse.

Taps are not what the person sees. Hex's next-letter odds and autocorrect repair many letter
slips, and a space read as a letter runs two words together. So each zone is also scored on
the logged words (words.py says what Hex did with each):
- a word whose taps the zones don't change keeps what the log says Hex did;
- a word Hex got wrong whose every tap now presses the key meant is made right (as often as
  Hex leaves an exactly typed word right for this person);
- a word Hex got right that now has a new slip stays right only as often as Hex repaired a word
  with one slip of that kind in this half of the log: a letter read as its neighbour, a space
  read as a letter (which also joins the next word to it), a letter read as a space, or a tap
  on another key. Few examples count as no repair (the "repairPrior" limit), so a break is
  never cheaper than the log shows;
- a word that was wrong and still has a slip counts as no change either way.
"words right" and "words wrong" are those expected counts. Simulate a key block's zones
together: they interact.

    python3 scripts/simulate_zones.py [EXPORT_DIR] space:top:+0.15 o:bottom:+0.1 [--half B]
    python3 scripts/simulate_zones.py [EXPORT_DIR] --config typing-config.mhexconfig [--half B]
    python3 scripts/simulate_zones.py [EXPORT_DIR] --sweep space:top [other zones held] [--half A]
    python3 scripts/simulate_zones.py [EXPORT_DIR] --repair [--half A]
"""
import json, os, sys, collections
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import hexlog, taps as T, geometry as G, words as W
from limits import LIMITS

SLIPS = ("letter", "spaceAsLetter", "letterAsSpace", "otherKey")


def slip(meant, key):
    """The kind of slip a tap meant for `meant` makes when it presses `key`."""
    if meant == "space":
        return "spaceAsLetter"
    if key == "space":
        return "letterAsSpace"
    return "letter" if key and len(key) == 1 and key.isalpha() else "otherKey"


def scorable(w):
    """Whether the log says what Hex did with this word: as words.scorable, and a word Hex or the
    person split in two (a space read as a letter) counts too."""
    bad = {"pick_bar", "changed_after_commit", "uncommitted", "revised_by_next_word", "merged_into_next", "swipe", "abandoned"}
    f = W.norm(w.get("final")).replace("'", "")
    return bool(f) and f.replace(" ", "").isalpha() and "  " not in f and bool(w.get("raw_full")) \
        and w.get("hex_saw") and w.get("known") and not (set(w["cls"]) & bad)


def units(export, half):
    """The logged words of one half, in order, with their taps and whether Hex got them right
    (None when the log can't say: a pick from the bar, a rewrite, a swipe)."""
    words = {w["id"]: w for w in W.load(export)[0]}
    out, at = [], {}
    for t in T.load(export, half):
        if t["word"] not in at:
            w = words.get(t["word"])
            right = None
            if w and scorable(w):
                right = 1 if w.get("hex_right") else 0
            at[t["word"]] = len(out)
            out.append({"id": t["word"], "session": t["session"], "taps": [], "right": right,
                        "weight": max(1, len(W.norm(w["final"]).split())) if w and w.get("final") else 1})
        out[at[t["word"]]]["taps"].append(t)
    return out


def _slips(u, keys):
    """The slips of a word's own taps with `keys` (one per tap) as [(tap index, kind)]."""
    return [(j, slip(t["meant"], k)) for j, (t, k) in enumerate(zip(u["taps"], keys)) if k != t["meant"]]


def _joined(us, i, keys_of):
    """Whether word i is run into the one before it: that word's last tap, meant as the space
    that ended it, pressed another key."""
    if i == 0 or us[i - 1]["session"] != us[i]["session"]:
        return False
    prev = us[i - 1]
    t = prev["taps"][-1]
    return t["meant"] == "space" and keys_of(i - 1)[-1] != "space"


def repair_rates(us):
    """Per kind of slip: how often Hex still got a word with exactly one such slip right
    (a run-in counts for both words it touches), shrunk toward Hex's own rate on the public
    recordings ("repair" in the limits) by "repairPrior" words; and how often it left an exactly
    typed word right."""
    n, ok = collections.Counter(), collections.Counter()
    now = lambda i: [t["now"] for t in us[i]["taps"]]
    for i, u in enumerate(us):
        if u["right"] is None:
            continue
        kinds = [k for _, k in _slips(u, now(i))] + (["spaceAsLetter"] if _joined(us, i, now) else [])
        if len(kinds) <= 1:
            k = kinds[0] if kinds else "exact"
            n[k] += 1; ok[k] += u["right"]
    Z = LIMITS["keys"]["zones"]
    prior, mean = Z["repairPrior"], Z["repair"]
    rates = {k: (ok[k] + prior * mean[k]) / (n[k] + prior) for k in SLIPS}
    rates["exact"] = ok["exact"] / n["exact"] if n["exact"] else 1.0
    return rates, n, ok


def simulate(export, zones, half):
    """(per zone {"taps", "gained", "lost", "wordsRight", "wordsWrong"} or {"skipped": reason},
    totals, per key gained/lost)."""
    layout = G.load_layout(export)
    base = G.base_regions(layout)
    regions, skipped, strips = G.zone_map(layout, zones)
    per = []
    for n, z in enumerate(zones):
        per.append({"skipped": skipped[n]} if skipped[n] else {"taps": 0, "gained": 0, "lost": 0, "wordsRight": 0.0, "wordsWrong": 0.0})
    total = {"taps": 0, "gained": 0, "lost": 0, "wordsRight": 0.0, "wordsWrong": 0.0}
    keys = collections.Counter()

    def owner_of(t):
        # Only a tap inside a zone's strips can press another key.
        return next((n for n in range(len(zones)) if not skipped[n] and any(G.contains(s[0], t["x"], t["y"]) for s in strips[n])), None)

    us = units(export, half)
    rates, _, _ = repair_rates(us)
    before, after, owner = [], [], []
    for u in us:
        b_, a_, o_ = [], [], []
        for t in u["taps"]:
            o = owner_of(t)
            kb = G.key_at(layout, base, t["x"], t["y"])
            ka = G.key_at(layout, regions, t["x"], t["y"]) if o is not None else kb
            if o is not None:
                per[o]["taps"] += 1; total["taps"] += 1
                if kb != t["meant"] and ka == t["meant"]:
                    per[o]["gained"] += 1; total["gained"] += 1; keys[(t["meant"], "gained")] += 1
                elif kb == t["meant"] and ka != t["meant"]:
                    per[o]["lost"] += 1; total["lost"] += 1; keys[(t["meant"], "lost")] += 1
            b_.append(kb); a_.append(ka); o_.append(o)
        before.append(b_); after.append(a_); owner.append(o_)
    for i, u in enumerate(us):
        own = [j for j in range(len(u["taps"])) if before[i][j] != after[i][j]]
        lead = i > 0 and us[i - 1]["session"] == u["session"] and before[i - 1][-1] != after[i - 1][-1]
        if (not own and not lead) or u["right"] is None:
            continue
        sb = dict(_slips(u, before[i])); sa = dict(_slips(u, after[i]))
        jb, ja = _joined(us, i, lambda k: before[k]), _joined(us, i, lambda k: after[k])
        new = [k for j, k in sa.items() if j not in sb] + (["spaceAsLetter"] if ja and not jb else [])
        if not new:
            clean = not sa and not ja
            delta = rates["exact"] * u["weight"] if clean and not u["right"] else 0.0
        else:
            p = u["right"]
            for k in new:
                p *= rates[k]
            delta = (p - u["right"]) * u["weight"]
        if not delta:
            continue
        causes = [owner[i][j] for j in own] + ([owner[i - 1][-1]] if lead else [])
        for o in causes:
            per[o]["wordsRight" if delta > 0 else "wordsWrong"] += abs(delta) / len(causes)
        total["wordsRight" if delta > 0 else "wordsWrong"] += abs(delta)
    for p in per + [total]:
        if "skipped" not in p:
            p["wordsRight"] = round(p["wordsRight"], 1); p["wordsWrong"] = round(p["wordsWrong"], 1)
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


def net_words(p):
    return round(p["wordsRight"] - p["wordsWrong"], 1)


def show(per, total, keys, zones):
    for z, p in zip(zones, per):
        name = "%s %s %+.2f" % (z["key"], z["edge"], z["move"])
        if "skipped" in p:
            print("  %-22s skipped: %s" % (name, p["skipped"]))
        else:
            print("  %-22s taps in its strip %3d, gained %3d, lost %3d | words right %5.1f, wrong %5.1f, net %+.1f" % (
                name, p["taps"], p["gained"], p["lost"], p["wordsRight"], p["wordsWrong"], net_words(p)))
    print("  total: taps %d, gained %d, lost %d | words right %.1f, wrong %.1f, net %+.1f" % (
        total["taps"], total["gained"], total["lost"], total["wordsRight"], total["wordsWrong"], net_words(total)))
    by = sorted({k for k, _ in keys})
    if by:
        print("  taps by meant key: " + ", ".join("%s +%d -%d" % (k, keys[(k, "gained")], keys[(k, "lost")]) for k in by))


def holds(p):
    """Whether a zone's numbers on the check half pass the rule (reference/typing-config.md, Zones)."""
    Z = LIMITS["keys"]["zones"]
    return p["taps"] >= Z["minCheckedTaps"] and net_words(p) >= Z["minNetCheckedWords"] and p["wordsWrong"] <= Z["maxWrongShare"] * p["wordsRight"]


def evidence(p):
    return {x: p[x] for x in ("taps", "gained", "lost", "wordsRight", "wordsWrong")}


if __name__ == "__main__":
    ex = hexlog.export_dir([a for a in sys.argv[1:] if os.path.isdir(a)])
    half = sys.argv[sys.argv.index("--half") + 1] if "--half" in sys.argv else None
    if "--repair" in sys.argv:
        rates, n, ok = repair_rates(units(ex, half))
        print("how often Hex still got a word right, half %s (words with exactly one slip of the kind):" % (half or "A+B"))
        print("  %-14s %6s %6s %6s" % ("slip", "words", "right", "rate"))
        for k in ("exact",) + SLIPS:
            print("  %-14s %6d %6d %6.2f" % (k, n[k], ok[k], rates[k]))
        print("  (each rate leans toward Hex's own on the public recordings, the limits' \"repair\", by %d words)" % LIMITS["keys"]["zones"]["repairPrior"])
        sys.exit(0)
    held = zones_from(sys.argv, ex)
    if "--sweep" in sys.argv:
        key, edge = sys.argv[sys.argv.index("--sweep") + 1].split(":")
        held = [z for z in held if not (z["key"] == key and z["edge"] == edge)]
        step, bound = LIMITS["keys"]["zones"]["step"], LIMITS["keys"]["zones"]["maxMove"]
        print("sweep %s %s on half %s (other zones held: %s)" % (key, edge, half or "A+B", held or "none"))
        print("  %6s %6s %6s %6s %6s %7s %7s %7s  %s" % ("move", "taps", "gained", "lost", "net", "w.right", "w.wrong", "w.net", "geometry"))
        n = int(round(bound / step))
        for i in [j for j in range(-n, n + 1) if j]:
            move = round(i * step, 2)
            per, total, _ = simulate(ex, held + [{"key": key, "edge": edge, "move": move}], half)
            p = per[-1]
            if "skipped" in p:
                print("  %+6.2f %50s  %s" % (move, "", p["skipped"]))
            else:
                print("  %+6.2f %6d %6d %6d %+6d %7.1f %7.1f %+7.1f" % (move, p["taps"], p["gained"], p["lost"], p["gained"] - p["lost"], p["wordsRight"], p["wordsWrong"], net_words(p)))
        sys.exit(0)
    if not held:
        raise SystemExit("give zones as key:edge:move, --config FILE, --sweep key:edge or --repair")
    per, total, keys = simulate(ex, held, half)
    print("zones on half %s:" % (half or "A+B"))
    show(per, total, keys, held)
    print("evidence json:", json.dumps([p if "skipped" in p else evidence(p) for p in per]))
