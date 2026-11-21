"""Synthetic German and Bokmål typing sets for the Android replay, made the way
2026-11-20-hex-typing-logs/bench_make.py made the English and Spanish ones: held-out Tatoeba
sentences (CC BY 2.0 FR), each tap sampled from the measured per-key offset and per-row spread
in that benchmark's input.json meta (no log rows), with 2% of spaces missed and space taps that
land on a letter typed as that letter. Keys: Android's rows (qwertzuiopü / asdfghjklöä / yxcvbnm,
qwertyuiopå / asdfghjkløæ / zxcvbnm) on a 402 pt layout with 11 keys a row (36 pt pitch) and the
English rows' heights and space bar. Letters without a key are typed as their base key (é with e);
German sentences with ß are left out (it has no key; it is held on s).

Writes OUTDIR/{de,nb}.jsonl (id, intended, typed, taps, want, ends, ios_shared="") and {de,nb}-dev.jsonl.
TATOEBA holds deu.tsv and nob.tsv from https://downloads.tatoeba.org/exports/per_language/.
Usage: python3 synthetic_de_nb.py INPUT.json TATOEBA OUTDIR"""
import json, os, random, re, sys, unicodedata

meta = json.load(open(sys.argv[1]))["meta"]
tatoeba, out_dir = sys.argv[2], sys.argv[3]
OFFSET = {k: tuple(v) for k, v in meta["offsets"].items()}
ROW_SD = {int(k): tuple(v) for k, v in meta["row_sd"].items()}
SPACE_MU, SPACE_SD = meta["space"][:2], meta["space"][2:]
SPACE_OMIT = meta["space_omit"]
PITCH, ROW_Y, TOP = 36.0, [66.5, 122.5, 178.5], 44.5
SPACE = (64.76, 212.0, 238.34, 45.0)
ROWS = {"de": ["qwertzuiopü", "asdfghjklöä", "yxcvbnm"], "nb": ["qwertyuiopå", "asdfghjkløæ", "zxcvbnm"]}
ALPHABET = {"de": "abcdefghijklmnopqrstuvwxyzäöü", "nb": "abcdefghijklmnopqrstuvwxyzæøåé"}
EN_ROWS = ["qwertyuiop", "asdfghjkl", "zxcvbnm"]


def geometry(lang):
    keys = {}
    for r, letters in enumerate(ROWS[lang]):
        for i, c in enumerate(letters):
            x = 21 + PITCH * i if len(letters) == 11 else 201 + (i - (len(letters) - 1) / 2) * PITCH
            keys[c] = (x, ROW_Y[r], r)
    return keys


def row_mean(row):
    letters = [c for c in EN_ROWS[row] if c in OFFSET]
    return (sum(OFFSET[c][0] for c in letters) / len(letters), sum(OFFSET[c][1] for c in letters) / len(letters))


def hit(keys, lang, x, y):
    if y >= ROW_Y[2] + 28:
        return "space" if SPACE[0] <= x < SPACE[0] + SPACE[2] else None
    row = 0 if y < ROW_Y[0] + 28 else 1 if y < ROW_Y[1] + 28 else 2
    letters = ROWS[lang][row]
    xs = [keys[c][0] for c in letters]
    if row == 2 and (x < xs[0] - PITCH / 2 or x >= xs[-1] + PITCH / 2):
        return None
    return min(letters, key=lambda c: abs(keys[c][0] - x))


def fold(lang, c):
    if c in "".join(ROWS[lang]):
        return c
    return unicodedata.normalize("NFD", c)[0]


def sample_letter(rng, keys, lang, ch):
    cx, cy, row = keys[ch]
    ox, oy = OFFSET.get(ch, row_mean(row))
    sx, sy = ROW_SD[row]
    for _ in range(20):
        x, y = rng.gauss(cx + ox, sx), max(rng.gauss(cy + oy, sy), TOP)
        k = hit(keys, lang, x, y)
        if k is not None:
            return x, y, k
    return cx, cy, ch


def sample_space(rng, keys, lang, force):
    for _ in range(20):
        x, y = rng.gauss(SPACE_MU[0], SPACE_SD[0]), rng.gauss(SPACE_MU[1], SPACE_SD[1])
        k = hit(keys, lang, x, y)
        if k == "space" or (not force and k is not None):
            return x, y, k
    return SPACE[0] + SPACE[2] / 2, SPACE[1] + SPACE[3] / 2, "space"


def sentences(lang):
    code = {"de": "deu", "nb": "nob"}[lang]
    word = re.compile(f"^[{ALPHABET[lang]}]+$")
    rows = []
    for line in open(os.path.join(tatoeba, f"{code}.tsv"), encoding="utf-8"):
        parts = line.rstrip("\n").split("\t")
        if len(parts) < 3:
            continue
        text = unicodedata.normalize("NFC", parts[2])
        if "'" in text or "\u2019" in text:
            continue
        ws = re.sub(r"[.,!?;:\"()\u201e\u201c\u201d\u201a\u2018\u00bb\u00ab\u2013\u2014\-]", " ", text).lower().split()
        if 3 <= len(ws) <= 14 and all(word.match(w) for w in ws):
            rows.append(ws)
    rng = random.Random({"de": 11, "nb": 13}[lang])
    rng.shuffle(rows)
    return rows[:2300]


for lang in ("de", "nb"):
    keys = geometry(lang)
    rng = random.Random({"de": 20261121, "nb": 20261122}[lang])
    rows = sentences(lang)
    for name, part in (("dev", rows[:300]), ("test", rows[300:2300])):
        path = os.path.join(out_dir, lang + (".jsonl" if name == "test" else "-dev.jsonl"))
        with open(path, "w") as f:
            for j, ws in enumerate(part):
                taps, typed, want, ends = [], [], [], []
                for i, w in enumerate(ws):
                    for ch in w:
                        x, y, k = sample_letter(rng, keys, lang, fold(lang, ch))
                        taps.append([round(x, 2), round(y, 2)]); typed.append(" " if k == "space" else k); want.append(ch)
                    ends.append(len(taps))
                    last = i == len(ws) - 1
                    if not last and rng.random() < SPACE_OMIT:
                        continue
                    x, y, k = sample_space(rng, keys, lang, last)
                    taps.append([round(x, 2), round(y, 2)]); typed.append(" " if k == "space" else k); want.append(" ")
                f.write(json.dumps({"id": f"{lang}-{name}-{j}", "intended": " ".join(ws), "typed": "".join(typed),
                                    "taps": taps, "want": "".join(want), "ends": ends, "ios_shared": ""}, ensure_ascii=False, separators=(",", ":")) + "\n")
    print(lang, len(rows), "sentences")
