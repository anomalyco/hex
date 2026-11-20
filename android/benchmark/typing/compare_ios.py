"""Score a newer iOS output in place. Only aggregate numbers for allowlisted synthetic test IDs leave the Studio."""
import json, re, sys
from pathlib import Path

def words(s):
    return [w for t in s.lower().split() if (w := re.sub(r"[.,!?;:]+$", "", t))]

def distance(ref, hyp):
    row = list(range(len(hyp) + 1))
    for i, a in enumerate(ref, 1):
        prior, row[0] = row[0], i
        for j, b in enumerate(hyp, 1):
            old = row[j]
            row[j] = min(row[j] + 1, row[j-1] + 1, prior + (a != b))
            prior = old
    return row[-1]

refs = {r["id"]: words(r["intended"]) for r in map(json.loads, Path(sys.argv[1]).read_text().splitlines())}
assert set(refs) == {"test-" + str(i) for i in range(2000)}
total = sum(map(len, refs.values()))
outputs = json.loads(Path(sys.argv[3]).read_text())["outputs"]
diffs = [distance(ref, words(outputs[i])) for i, ref in refs.items()]
print(f"wer\t{sys.argv[2]}\t{sum(diffs)}\t{total}\t{100 * sum(diffs) / total:.4f}\t{diffs.count(0)}/2000")
