"""The held-out split. Every session is in half A or B (a stable hash of its id). Find things
on one half and check them on the other: a field goes into the profile only when its evidence
holds on both halves (same direction, roughly the same size). This prints the halves' sizes.

    python3 scripts/split.py [EXPORT_DIR]
    then e.g.  python3 scripts/touch_offsets.py --half A   and   --half B
"""
import os, sys, collections
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import hexlog, words as W

ex = hexlog.export_dir(sys.argv[1:])
words, LAY, sessions = W.load(ex)
c = collections.Counter()
for s in sessions:
    c[("sessions", W.half_of(s))] += 1
for w in words:
    c[("words", w["half"])] += 1
    if W.scorable(w):
        c[("scorable", w["half"])] += 1
for k in ("sessions", "words", "scorable"):
    print("%-9s A %5d   B %5d" % (k, c[(k, "A")], c[(k, "B")]))
