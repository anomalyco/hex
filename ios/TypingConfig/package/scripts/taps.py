"""Every tap whose meaning is known, mapped onto the keyboard the person uses now.

For each word whose final spelling is known (words.py), its taps are lined up with the letters
(and spaces) it ended as, so each tap has the key it was **meant** for, not just the one it
pressed. The space that finished a word was meant as a space, unless the person then finished
the word longer from where that space went in ("o" then fixed to "on"): that tap was meant for
the next letter, and pressed the space bar instead. Each tap's landing point is
mapped onto state/phone.json's keys: it keeps its place on the keycap it landed on, scaled to
that keycap now (fingers aim at keycaps), so taps from older layouts count too.
Writes <export>/out/taps.json; letters and "space" only, no words.

    python3 scripts/taps.py [EXPORT_DIR]
"""
import json, os, sys, collections
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import hexlog, words as W, geometry as G


def label(L):
    m = L.get("metrics") or {}
    return "%sx%s keys %s" % (L.get("width"), L.get("height"), m.get("keyHeight"))


def build(export):
    words, LAY, _ = W.load(export)
    target = G.load_layout(export)
    tkeys = {k["id"]: k for k in target["keys"] if k["row"] >= 0}
    base = G.base_regions(target)
    out = []
    dropped = collections.Counter()

    def mapped(t):
        L = LAY.get(t["layout"])
        if not L:
            dropped["layout not logged"] += 1; return None
        s0 = t["samples"][0]; x, y = s0[1], s0[2]
        under = None
        for k in L["keys"]:
            rects = [k["hitFrame"]] + ([k["gapHitFrame"]] if k.get("gapHitFrame") else [])
            if any(G.contains(r, x, y) for r in rects):
                under = k; break
        if under is None or under["id"] not in tkeys:
            dropped["off the keys now"] += 1; return None
        of, nf = under["frame"], tkeys[under["id"]]["frame"]
        nx = nf[0] + nf[2] / 2 + (x - of[0] - of[2] / 2) * nf[2] / of[2]
        ny = nf[1] + nf[3] / 2 + (y - of[1] - of[3] / 2) * nf[3] / of[3]
        return nx, ny, under["id"], L

    def add(t, meant, w):
        m = mapped(t)
        if m is None: return
        nx, ny, under, L = m
        out.append({"meant": meant, "x": round(nx, 2), "y": round(ny, 2), "pressed": t["key0"], "under": under,
                    "now": G.key_at(target, base, nx, ny), "half": w["half"], "session": w["session"],
                    "source": label(L), "word": w["id"]})

    for w in words:
        if w["swiped"] or not w.get("final"):
            continue
        final = W.norm(w["final"])
        taps = w.get("error_kept_taps") or w["taps"]
        raw = w.get("raw_full") or ""
        if taps and raw and len(raw) == len(taps) and all(t is not None for t in taps):
            for op, i, j in W.align(raw, final):
                if op in ("m", "s") and (final[j].isalpha() or final[j] == " "):
                    add(taps[i], "space" if final[j] == " " else final[j], w)
        ct = w.get("commit_touch")
        if ct and ct.get("key0") == "space" and w.get("commit_src") is not None and not w.get("merged_into"):
            committed = W.norm(w.get("committed") or "")
            if committed and len(final) > len(committed) and final.startswith(committed) and final[len(committed)].isalpha():
                add(ct, final[len(committed)], w)
            else:
                add(ct, "space", w)
    os.makedirs(os.path.join(export, "out"), exist_ok=True)
    json.dump({"layout": target["context"], "taps": out, "dropped": dropped}, open(os.path.join(export, "out", "taps.json"), "w"))
    return out, dropped


_cache = {}


def load(export, half=None):
    """The taps (built if needed), one half's or all."""
    p = os.path.join(export, "out", "taps.json")
    key = (p, os.path.getmtime(p) if os.path.exists(p) else None)
    if key not in _cache:
        _cache.clear()
        _cache[key] = json.load(open(p))["taps"] if os.path.exists(p) else build(export)[0]
    return [t for t in _cache[key] if half is None or t["half"] == half]


if __name__ == "__main__":
    ex = hexlog.export_dir(sys.argv[1:])
    taps, dropped = build(ex)
    by = collections.Counter((t["half"], "space" if t["meant"] == "space" else "letter") for t in taps)
    src = collections.Counter(t["source"] for t in taps)
    print("%d taps with a known meaning -> %s" % (len(taps), os.path.join(ex, "out", "taps.json")))
    print("  half A: %d letters, %d spaces; half B: %d letters, %d spaces" % (by[("A", "letter")], by[("A", "space")], by[("B", "letter")], by[("B", "space")]))
    print("  logged on (width x height, key height):", dict(src))
    print("  not mapped:", dict(dropped))
    wrong = sum(1 for t in taps if t["now"] != t["meant"])
    print("  pressing another key than meant on today's keys: %d (%.1f%%)" % (wrong, 100 * wrong / max(1, len(taps))))
