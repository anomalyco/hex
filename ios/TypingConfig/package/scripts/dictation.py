"""Dictations: how many, what became of them (kept, edited, ...), and the word pairs the person
fixed (heard -> meant), counted. Single words only.

    python3 scripts/dictation.py [EXPORT_DIR] [--half A|B]
"""
import json, os, sys, collections
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import hexlog, words as W

ex = hexlog.export_dir(sys.argv[1:])
half = W.half_arg(sys.argv)
out = collections.Counter(); pairs = collections.Counter(); n = 0; eng = collections.Counter()
for f in hexlog.files_in(ex):
    for line in open(f, encoding="utf-8"):
        e = json.loads(line)
        if half and W.half_of(e["session"]) != half:
            continue
        if e["type"] == "dictation":
            n += 1; eng[(e.get("source"), e.get("engine"), e.get("style"))] += 1
        elif e["type"] == "dictation_outcome":
            out[e.get("outcome")] += 1
            for p in e.get("pairs") or []:
                h, m = (p.get("heard") or p.get("inserted") or "").lower(), (p.get("meant") or "").lower()
                if h and m and h != m and len(h.split()) <= 2 and len(m.split()) <= 2:
                    pairs[(p.get("kind"), h, m)] += 1
print("dictations: %d; by source, engine, style: %s" % (n, dict(eng)))
print("outcomes:", dict(out))
print("word fixes after dictation (kind, heard -> meant), times:")
for (k, h, m), c in pairs.most_common(40):
    print("  %-10s %-18s -> %-18s %d" % (k, h, m, c))
