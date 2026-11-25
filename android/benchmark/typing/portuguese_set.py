"""Portuguese held-out typing set, made exactly as polish_set.py makes Polish's: held-out Tatoeba
sentences (CC BY 2.0 FR, or CC0 where released so), each tap sampled from the measured per-key
offset and per-row spread in the English benchmark's input.json meta, 2% of spaces missed, a space
tap landing on a letter typed as that letter. Hex's Portuguese keys are QWERTY at English's 39.5 pt
pitch; á à â ã ç é ê í ó ô õ ú have no key of their own (each is held on its base key), so they are
typed as their base key.

Brazilian Portuguese only: Tatoeba doesn't tell Brazil from Portugal, so a sentence is taken from a
contributor whose sentences use spellings only Brazil writes at least three times as often as ones
only Portugal writes (twenty or more), and never when it has one of Portugal's itself
(build-layout-model.py's PT_MARKERS, plus "estar a" before an infinitive). "--portugal" makes the
mirror set from Portugal's contributors instead. Sentences with an apostrophe or a hyphen are left
out: the synthetic typist has no key for either.

Writes OUTDIR/pt.jsonl (2,000 test rows, as synthetic_sets.py) and pt-dev.jsonl (300), and
OUTDIR/pt.tsv.gz in held-out/'s format: Tatoeba ID, contributor, reference with the source's case,
the literal hit keys with Shift where the reference has a capital (including the final space), and
the taps as "x,y". With --portugal the files are pt-pt.*.
INPUT.json is the English benchmark's (2026-11-20-hex-typing-logs), as for synthetic_sets.py;
por_sentences_detailed.tsv.bz2 is https://downloads.tatoeba.org/exports/per_language/por/.
Usage: python3 portuguese_set.py INPUT.json por_sentences_detailed.tsv[.bz2] OUTDIR [--portugal]"""
import bz2, collections, gzip, importlib, json, os, random, re, sys, unicodedata

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
layout = importlib.import_module("build-layout-model")

meta = json.load(open(sys.argv[1]))["meta"]
source, out_dir = sys.argv[2], sys.argv[3]
PORTUGAL = sys.argv[4:] == ["--portugal"]
NAME = "pt-pt" if PORTUGAL else "pt"
OFFSET = {k: tuple(v) for k, v in meta["offsets"].items()}
ROW_SD = {int(k): tuple(v) for k, v in meta["row_sd"].items()}
SPACE_MU, SPACE_SD = meta["space"][:2], meta["space"][2:]
SPACE_OMIT = meta["space_omit"]
ROW_Y, TOP = [66.5, 122.5, 178.5], 44.5
SPACE = (64.76, 212.0, 238.34, 45.0)
ROWS = ["qwertyuiop", "asdfghjkl", "zxcvbnm"]
ALPHABET = layout.PT_LETTERS
PITCH = 39.5
SEEDS = (2026, 1124)
MARKERS = {"br": [layout.PT_MARKERS["br"], re.compile(r"\b(?:estou|está|estás|estão|estava|estavam|estamos)\s+\w+ndo\b")],
           "pt": [layout.PT_MARKERS["pt"], re.compile(r"\b(?:estou|está|estás|estão|estava|estavam|estamos)\s+a\s+\w+[aei]r\b")]}


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
    return c if c in "".join(ROWS) else unicodedata.normalize("NFD", c)[0]


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


def has(region, text):
    low = text.lower()
    return any(m.search(low) for m in MARKERS[region])


def sentences():
    word = re.compile(f"^[{ALPHABET}]+$")
    rows, votes = [], collections.defaultdict(lambda: [0, 0])
    opener = bz2.open if source.endswith(".bz2") else open
    for line in opener(source, "rt", encoding="utf-8"):
        parts = line.rstrip("\n").split("\t")
        if len(parts) < 4 or parts[3] in ("", "\\N"):
            continue
        text = unicodedata.normalize("NFC", parts[2])
        votes[parts[3]][0] += has("br", text)
        votes[parts[3]][1] += has("pt", text)
        if "'" in text or "\u2019" in text or "-" in text:
            continue
        cased = re.sub(r"[.,!?;:\"()\u201c\u201d\u2018\u00bb\u00ab\u2013\u2014]", " ", text).split()
        ws = [w.lower() for w in cased]
        if 3 <= len(ws) <= 14 and all(word.match(w) for w in ws):
            rows.append((parts[0], parts[3], cased, ws, text))
    mine, other = (1, 0) if PORTUGAL else (0, 1)
    owners = {o for o, v in votes.items() if v[mine] >= 20 and v[mine] >= 3 * v[other]}
    rows = [r[:4] for r in rows if r[1] in owners and not has("br" if PORTUGAL else "pt", r[4])]
    print(NAME, len(owners), "contributors,", len(rows), "eligible sentences")
    rng = random.Random(SEEDS[0])
    rng.shuffle(rows)
    return rows[:2300]


keys = geometry()
rng = random.Random(SEEDS[1])
rows = sentences()
tsv = []
for name, part in (("dev", rows[:300]), ("test", rows[300:2300])):
    path = os.path.join(out_dir, NAME + (".jsonl" if name == "test" else "-dev.jsonl"))
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
            f.write(json.dumps({"id": f"{NAME}-{name}-{j}", "intended": " ".join(ws), "typed": "".join(typed),
                                "taps": taps, "want": "".join(want), "ends": ends, "ios_shared": ""}, ensure_ascii=False, separators=(",", ":")) + "\n")
            if name == "test":
                tsv.append("\t".join([sid, owner, " ".join(cased), "".join(shown), " ".join(f"{x},{y}" for x, y in taps)]))
with open(os.path.join(out_dir, NAME + ".tsv.gz"), "wb") as raw, gzip.GzipFile("", "wb", fileobj=raw, mtime=0) as g:
    g.write(("\n".join(tsv) + "\n").encode("utf-8"))
print(NAME, len(rows), "sentences;", len(tsv), "test rows")
