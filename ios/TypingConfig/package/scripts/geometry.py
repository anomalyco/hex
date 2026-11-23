"""Hex's keys as the export's state/phone.json describes them, and typing-config zones compiled
against them exactly as the phone does (HEXShared/FullKeyboard/TypingConfig/KeyboardZoneMap.swift).
Standard library only. Rects are [x, y, width, height] in points, y down."""
import json, math, os
from limits import LIMITS

EPS = 0.01


def load_layout(export):
    """The keyboard the person uses now (state/phone.json "layout")."""
    p = os.path.join(export, "state", "phone.json")
    if not os.path.exists(p):
        raise SystemExit("no state/phone.json: the export has no keyboard to tune against")
    layout = json.load(open(p))["layout"]
    if "keys" not in layout:
        raise SystemExit("state/phone.json has no layout.keys")
    return layout


def area(r):
    return r[2] * r[3]


def inter(a, b):
    x0, y0 = max(a[0], b[0]), max(a[1], b[1])
    x1, y1 = min(a[0] + a[2], b[0] + b[2]), min(a[1] + a[3], b[1] + b[3])
    return [x0, y0, x1 - x0, y1 - y0] if x1 > x0 and y1 > y0 else None


def overlaps(a, b):
    i = inter(a, b)
    return i is not None and i[2] > EPS and i[3] > EPS


def subtract(hole, r):
    i = inter(r, hole)
    if i is None or i[2] <= EPS or i[3] <= EPS:
        return [r]
    pieces = [[r[0], r[1], r[2], i[1] - r[1]], [r[0], i[1] + i[3], r[2], r[1] + r[3] - i[1] - i[3]],
              [r[0], i[1], i[0] - r[0], i[3]], [i[0] + i[2], i[1], r[0] + r[2] - i[0] - i[2], i[3]]]
    return [p for p in pieces if p[2] > EPS and p[3] > EPS]


def contains(r, x, y):
    return r[0] <= x < r[0] + r[2] and r[1] <= y < r[1] + r[3]


def base_regions(layout):
    return [[k["hitFrame"]] + ([k["gapHitFrame"]] if k.get("gapHitFrame") else []) for k in layout["keys"]]


def key_at(layout, regions, x, y):
    """The id of the key a tap at (x, y) presses with these touch areas (Hex's hitTest)."""
    for k, rects in zip(layout["keys"], regions):
        if any(contains(r, x, y) for r in rects):
            return k["id"]
    return None


def _edges_for(key):
    edges = LIMITS["keys"]["zones"]["edges"]
    letter = len(key) == 1 and key.isalpha()
    return edges.get("letter" if letter else key)


def _strips(layout, base, zone):
    key, edge, move = zone["key"], zone["edge"], zone["move"]
    allowed = _edges_for(key)
    if not allowed or edge not in allowed:
        return [], "edgeNotAllowed"
    keys = layout["keys"]
    targets = [i for i, k in enumerate(keys) if k["row"] >= 0 and k["row"] != -2 and k["id"] == key]
    if not targets:
        return [], "keyNotOnKeyboard"
    bound = LIMITS["keys"]["zones"]["maxMove"]
    move = max(-bound, min(bound, move))
    vertical = edge in ("top", "bottom")
    d = abs(move) * (layout["keyHeight"] if vertical else layout["letterWidth"])
    if d <= EPS:
        return [], None
    ta = layout["touchArea"]
    out = []
    for i in targets:
        rects = base[i]
        minx = min(r[0] for r in rects); miny = min(r[1] for r in rects)
        maxx = max(r[0] + r[2] for r in rects); maxy = max(r[1] + r[3] for r in rects)
        line = {"top": miny, "bottom": maxy, "left": minx, "right": maxx}[edge]
        outer = {"top": line <= ta[1] + EPS, "bottom": line >= ta[1] + ta[3] - EPS,
                 "left": line <= ta[0] + EPS, "right": line >= ta[0] + ta[2] - EPS}[edge]
        if outer:
            return [], "edgeNotAllowed"
        def on_edge(r):
            return {"top": abs(r[1] - line), "bottom": abs(r[1] + r[3] - line), "left": abs(r[0] - line),
                    "right": abs(r[0] + r[2] - line)}[edge] < EPS
        for s in [r for r in rects if on_edge(r)]:
            if move > 0:
                grown = {"top": [s[0], line - d, s[2], d], "bottom": [s[0], line, s[2], d],
                         "left": [line - d, s[1], d, s[3]], "right": [line, s[1], d, s[3]]}[edge]
                c = inter(grown, ta)
                if c and c[2] > EPS and c[3] > EPS:
                    out.append((c, i))
            else:
                strip = {"top": [s[0], line, s[2], d], "bottom": [s[0], line - d, s[2], d],
                         "left": [line, s[1], d, s[3]], "right": [line - d, s[1], d, s[3]]}[edge]
                for j, rs in enumerate(base):
                    if j == i or keys[j]["row"] < 0:
                        continue
                    for r in rs:
                        touching = {"top": abs(r[1] + r[3] - line), "bottom": abs(r[1] - line),
                                    "left": abs(r[0] + r[2] - line), "right": abs(r[0] - line)}[edge] < EPS
                        if not touching:
                            continue
                        if vertical:
                            x0, x1 = max(strip[0], r[0]), min(strip[0] + strip[2], r[0] + r[2])
                            piece = [x0, strip[1], x1 - x0, strip[3]]
                        else:
                            y0, y1 = max(strip[1], r[1]), min(strip[1] + strip[3], r[1] + r[3])
                            piece = [strip[0], y0, strip[2], y1 - y0]
                        if piece[2] > EPS and piece[3] > EPS:
                            out.append((piece, j))
    return out, None


def keep_rect(k):
    keep = LIMITS["keys"]["zones"]["keepFromCentre"]
    f = k["frame"]
    cx, cy = f[0] + f[2] / 2, f[1] + f[3] / 2
    return [cx - keep * f[2], cy - keep * f[3], 2 * keep * f[2], 2 * keep * f[3]]


def covers(rects, target):
    a = sum(area(i) for i in (inter(r, target) for r in rects) if i)
    return a >= area(target) - 0.5


def zone_map(layout, zones):
    """(touch areas per key, and per zone None or the reason it was skipped), as on the phone."""
    keys = layout["keys"]
    base = base_regions(layout)
    skipped = [None] * len(zones)
    strips = [[] for _ in zones]
    for n, z in enumerate(zones):
        strips[n], skipped[n] = _strips(layout, base, z)
    for a in range(len(zones)):
        for b in range(a + 1, len(zones)):
            if skipped[a] is None and skipped[b] is None and any(overlaps(s[0], t[0]) for s in strips[a] for t in strips[b]):
                skipped[a] = skipped[b] = "twoZonesClaimPoint"
    while True:
        regions = [list(r) for r in base]
        for n in range(len(zones)):
            if skipped[n] is not None:
                continue
            for rect, owner in strips[n]:
                for k in range(len(regions)):
                    if k != owner:
                        regions[k] = [p for r in regions[k] for p in subtract(rect, r)]
                pieces = [rect]
                for own in regions[owner]:
                    pieces = [p for q in pieces for p in subtract(own, q)]
                regions[owner] += pieces
        small = [k for k in range(len(keys)) if keys[k]["row"] >= 0 and keys[k]["frame"][2] > 0 and not covers(regions[k], keep_rect(keys[k]))]
        culprits = [n for n in range(len(zones)) if skipped[n] is None and any(overlaps(s[0], keep_rect(keys[k])) for k in small for s in strips[n])]
        if not small or not culprits:
            return regions, skipped, strips
        for n in culprits:
            skipped[n] = "keyTooSmall"


def keycap(layout, key_id):
    for k in layout["keys"]:
        if k["id"] == key_id:
            return k["frame"]
    return None


def letter_ids(layout):
    return [k["id"] for k in layout["keys"] if len(k["id"]) == 1 and k["id"].isalpha() and k["row"] >= 0]


def parse_zone(text):
    """"space:top:+0.15" -> {"key": "space", "edge": "top", "move": 0.15}."""
    key, edge, move = text.rsplit(":", 2)
    return {"key": key, "edge": edge, "move": float(move)}
