"""Polish held-out typing set, made exactly as android/benchmark/typing/synthetic_sets.py makes
German, Bokmål, French and Italian's: held-out Tatoeba sentences (CC BY 2.0 FR, or CC0 where
released so), each tap sampled from the measured per-key offset and per-row spread in the
English benchmark's input.json meta, 2% of spaces missed, a space tap landing on a letter typed
as that letter. Hex's Polish keys are QWERTY at English's 39.5 pt pitch; ą ć ę ł ń ó ś ź ż have
no key of their own (each is held on its base key), so they are typed as their base key (ł as l).

Writes OUTDIR/pl.jsonl (2,000 test rows, as synthetic_sets.py) and pl-dev.jsonl (300), and
OUTDIR/pl.tsv.gz in held-out/'s format: Tatoeba ID, contributor, reference with the source's
case, the literal hit keys with Shift where the reference has a capital (including the final
space), and the taps as "x,y".
INPUT.json is the English benchmark's (2026-11-20-hex-typing-logs), as for synthetic_sets.py;
pol_sentences_detailed.tsv.bz2 is https://downloads.tatoeba.org/exports/per_language/pol/.
Usage: python3 polish_set.py INPUT.json pol_sentences_detailed.tsv[.bz2] OUTDIR"""
import bz2, gzip, json, os, random, re, sys, unicodedata

meta = json.load(open(sys.argv[1]))["meta"]
source, out_dir = sys.argv[2], sys.argv[3]
OFFSET = {k: tuple(v) for k, v in meta["offsets"].items()}
ROW_SD = {int(k): tuple(v) for k, v in meta["row_sd"].items()}
SPACE_MU, SPACE_SD = meta["space"][:2], meta["space"][2:]
SPACE_OMIT = meta["space_omit"]
ROW_Y, TOP = [66.5, 122.5, 178.5], 44.5
SPACE = (64.76, 212.0, 238.34, 45.0)
ROWS = ["qwertyuiop", "asdfghjkl", "zxcvbnm"]
ALPHABET = "abcdefghijklmnopqrstuvwxyząćęłńóśźż"
PITCH = 39.5
SEEDS = (23, 20261125)


def geometry():
    keys = {}
    for r, letters in enumerate(ROWS):
        for i, c in enumerate(letters):
            keys[c] = (201 + (i - (len(letters) - 1) / 2) * PITCH, ROW_Y[r], r)
    return keys


def hit(keys, x, y):
    if y >= ROW_Y[2] + 28:
        return "space" if SPACE[0] <= x < SPACE[0] + SPACE[2] else None
    row = 0 if y < ROW_Y[0] + 28 else 1 if y < ROW_Y[1] + 28 else 2
    letters = ROWS[row]
    xs = [keys[c][0] for c in letters]
    if row == 2 and (x < xs[0] - PITCH / 2 or x >= xs[-1] + PITCH / 2):
        return None
    return min(letters, key=lambda c: abs(keys[c][0] - x))


def fold(c):
    if c in "".join(ROWS):
        return c
    return "l" if c == "ł" else unicodedata.normalize("NFD", c)[0]


def sample_letter(rng, keys, ch):
    cx, cy, row = keys[ch]
    ox, oy = OFFSET[ch]
    sx, sy = ROW_SD[row]
    for _ in range(20):
        x, y = rng.gauss(cx + ox, sx), max(rng.gauss(cy + oy, sy), TOP)
        k = hit(keys, x, y)
        if k is not None:
            return x, y, k
    return cx, cy, ch


def sample_space(rng, keys, force):
    for _ in range(20):
        x, y = rng.gauss(SPACE_MU[0], SPACE_SD[0]), rng.gauss(SPACE_MU[1], SPACE_SD[1])
        k = hit(keys, x, y)
        if k == "space" or (not force and k is not None):
            return x, y, k
    return SPACE[0] + SPACE[2] / 2, SPACE[1] + SPACE[3] / 2, "space"


def sentences():
    word = re.compile(f"^[{ALPHABET}]+$")
    rows = []
    opener = bz2.open if source.endswith(".bz2") else open
    for line in opener(source, "rt", encoding="utf-8"):
        parts = line.rstrip("\n").split("\t")
        if len(parts) < 4:
            continue
        text = unicodedata.normalize("NFC", parts[2])
        if "'" in text or "\u2019" in text:
            continue
        cased = re.sub(r"[.,!?;:\"()\u201e\u201c\u201d\u201a\u2018\u00bb\u00ab\u2013\u2014\-]", " ", text).split()
        ws = [w.lower() for w in cased]
        if 3 <= len(ws) <= 14 and all(word.match(w) for w in ws):
            rows.append((parts[0], parts[3], cased, ws))
    rng = random.Random(SEEDS[0])
    rng.shuffle(rows)
    return rows[:2300]


keys = geometry()
rng = random.Random(SEEDS[1])
rows = sentences()
tsv = []
for name, part in (("dev", rows[:300]), ("test", rows[300:2300])):
    path = os.path.join(out_dir, "pl.jsonl" if name == "test" else "pl-dev.jsonl")
    with open(path, "w") as f:
        for j, (sid, owner, cased, ws) in enumerate(part):
            taps, typed, want, ends, shown = [], [], [], [], []
            for i, (w, cw) in enumerate(zip(ws, cased)):
                for ch, cc in zip(w, cw):
                    x, y, k = sample_letter(rng, keys, fold(ch))
                    taps.append([round(x, 2), round(y, 2)]); typed.append(" " if k == "space" else k); want.append(ch)
                    shown.append(" " if k == "space" else (k.upper() if cc.isupper() else k))
                ends.append(len(taps))
                last = i == len(ws) - 1
                if not last and rng.random() < SPACE_OMIT:
                    continue
                x, y, k = sample_space(rng, keys, last)
                taps.append([round(x, 2), round(y, 2)]); typed.append(" " if k == "space" else k); want.append(" ")
                shown.append(" " if k == "space" else k)
            f.write(json.dumps({"id": f"pl-{name}-{j}", "intended": " ".join(ws), "typed": "".join(typed),
                                "taps": taps, "want": "".join(want), "ends": ends, "ios_shared": ""}, ensure_ascii=False, separators=(",", ":")) + "\n")
            if name == "test":
                tsv.append("\t".join([sid, owner or "\\N", " ".join(cased), "".join(shown), " ".join(f"{x},{y}" for x, y in taps)]))
with open(os.path.join(out_dir, "pl.tsv.gz"), "wb") as raw, gzip.GzipFile("", "wb", fileobj=raw, mtime=0) as g:
    g.write(("\n".join(tsv) + "\n").encode("utf-8"))
print("pl", len(rows), "sentences;", len(tsv), "test rows")
