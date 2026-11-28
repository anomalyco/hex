"""How long things take: letter taps (down to up), the gap between taps, long presses that
opened accents (and whether one was picked), space-bar holds that started the trackpad (and
whether the cursor moved), shift double-taps, delete held to repeat, and suggestion-strip
taps that came soon after a key. Compare these with Hex's constants (reference/profile-schema.md).

    python3 scripts/timings.py [EXPORT_DIR] [--half A|B]
"""
import json, os, sys, collections
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import hexlog, words as W

ex = hexlog.export_dir(sys.argv[1:])
half = W.half_arg(sys.argv)
dur = []; gaps = []; accents = collections.Counter(); track = collections.Counter(); shift_gaps = []
held = collections.Counter(); strip_after = []; acc_hold = []
for f in hexlog.files_in(ex):
    down = {}; last_up = {}; last_shift = {}; last_key_down = {}; open_acc = {}; tp = {}
    for line in open(f, encoding="utf-8"):
        e = json.loads(line); s = e["session"]
        if half and W.half_of(s) != half:
            continue
        if e["type"] == "touch":
            k = e.get("key")
            if e["phase"] == "began":
                down[(s, e["id"])] = (e["ts"], k)
                if k and len(k) == 1 and k.isalpha():
                    if s in last_up and 0 < e["ts"] - last_up[s] < 2000:
                        gaps.append(e["ts"] - last_up[s])
                    last_key_down[s] = e["mt"]
                if k == "shift":
                    if s in last_shift and e["ts"] - last_shift[s] < 1000:
                        shift_gaps.append(e["ts"] - last_shift[s])
                    last_shift[s] = e["ts"]
            elif e["phase"] in ("ended", "cancelled") and (s, e["id"]) in down:
                t0, k0 = down.pop((s, e["id"]))
                if k0 and len(k0) == 1 and k0.isalpha():
                    dur.append(e["ts"] - t0); last_up[s] = e["ts"]
                    if (s, e["id"]) in open_acc:
                        acc_hold.append(e["ts"] - t0)
        elif e["type"] == "ui":
            a = e.get("action")
            if a == "accent_open": open_acc[(s, e.get("id"))] = True; accents["opened"] += 1
            elif a == "accent_pick": accents["picked"] += 1
            elif a == "accent_cancel": accents["cancelled"] += 1
            elif a == "trackpad_start": track["started"] += 1
            elif a == "trackpad_end": track["moved the cursor" if e.get("offset") else "moved nothing"] += 1
        elif e["type"] == "edit":
            if e.get("op") == "delete":
                held[e.get("src")] += 1
            if e.get("op") == "suggestion" and s in last_key_down:
                strip_after.append(e["mt"] - last_key_down[s])


def hist(v, edges):
    c = collections.Counter()
    for x in v:
        b = next((f"<{e}" for e in edges if x < e), f">={edges[-1]}")
        c[b] += 1
    return {k: c[k] for k in [f"<{e}" for e in edges] + [f">={edges[-1]}"] if c[k]}


def pct(v, ps=(0.5, 0.9, 0.95, 0.99)):
    if not v: return "-"
    q = sorted(v); return " ".join("p%d %.0f" % (p * 100, q[int(p * (len(q) - 1))]) for p in ps)


print("letter taps, finger down to up (ms): n %d, %s" % (len(dur), pct(dur)))
print("   histogram:", hist(dur, [50, 100, 150, 200, 250, 300, 360, 500]))
print("   (accent popups open at 360 ms held: accentHoldDelay)")
print("gap between a letter's lift and the next letter (ms): n %d, %s" % (len(gaps), pct(gaps)))
print("accent popups:", dict(accents), "; held for (ms):", sorted(int(x) for x in acc_hold)[:30])
print("space-bar trackpad (starts after 350 ms held: trackpadHoldDelay):", dict(track))
print("shift taps less than 1 s apart (ms; caps lock is two within 300 ms: doubleTapWindow):", sorted(int(x) for x in shift_gaps))
print("deletes by source (key, repeat = held 360 ms then every 75 ms, repeat_fast after 0.85 s):", dict(held))
print("suggestion taps, ms after the last letter went down (the strip ignores touches within 250 ms of typing: stripTouchGuard):", hist(strip_after, [250, 400, 600, 1000]))
