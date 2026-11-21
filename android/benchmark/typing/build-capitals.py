#!/usr/bin/env python3
"""Build a keyboard language's capitals sidecar (<lang>.capitals) from Google Books Ngram v3
1-grams (2020-02-17, CC BY 3.0), the corpus its .hextm model was counted from.

    python3 build-capitals.py en MODEL.hextm WORKDIR OUTPUT.capitals scowl-2020.12.07.tar.gz
    python3 build-capitals.py {es|fr|it} MODEL.hextm WORKDIR OUTPUT.capitals

Every shard is streamed (curl | gzip -dc, never stored) and its 1980-2019 counts of all-lowercase
tokens and of tokens with only the first letter capitalised are cached in WORKDIR, so a rerun
resumes and a rule change renders from WORKDIR/counts.tsv in seconds. Tagged tokens ("_") and
other casings are ignored.

Each line is word<TAB>percent (the capitalised share, rounded half up), lowercase, sorted by its
UTF-8 bytes, with a trailing newline, as German's de.capitals:
- a model word whose share is at least 50% over at least 40 uses (the keyboard capitalises
  it when typed in lowercase from 95%);
- a word of three letters or more outside the model that the books capitalise at least 90% of
  the time and use at least once per million words: names and places ("tom", "olivia",
  "tokio"), which the keyboard then takes for real words, as iOS's spelling dictionary does, and
  capitalises from 95%. Rarer ones ("goyo", "eby") are more often a typo of a common word. Two model
  words run together whose pair is ten times as common ("iwant", "apartir") are left out.

The keyboard capitalises a word typed in lowercase only where iOS's dictionary would know it only
with a capital, so a word that is also an ordinary lowercase word is held at 94% (listed, never
capitalised on its own): in English, one SCOWL (sizes through 70, the lists en.valid comes from)
spells in lowercase ("united", "jersey", "china"); in Spanish, French and Italian, a model word
under 98% ("lima", "granada", "san"). In French and Italian, a word whose accented spelling is a
commoner model word is held, so "ecole" isn't written "Ecole" (books drop accents on capitals:
"Ecole" is "école").
"""
import hashlib, math, multiprocessing, os, re, struct, subprocess, sys, tarfile, unicodedata

BASE = "https://storage.googleapis.com/books/ngrams/books/20200217"
FILES = {"en": ("eng", 24), "es": ("spa", 3), "fr": ("fre", 6), "it": ("ita", 2)}
LETTERS = {"en": "a-z", "es": "a-zñáéíóúü", "fr": "a-zàâçéèêëîïôùûüÿœæ", "it": "a-zàèéìíîòóùú"}
FIRST_YEAR, LAST_YEAR = 1980, 2019
MIN_TOTAL = 40
SOMETIMES = 50
NAME_SHARE = 90
NAME_PER_MILLION = 1
ALWAYS, HELD = 95, 94
JOINED = 10
MODEL_ALWAYS = {"en": 95, "es": 98, "fr": 98, "it": 98}
SCOWL_SHA256 = "5587667caa20c4891390c2d42dbb4d5c4c3f41bee77af1457ece3ba23fb859cc"


def scowl_lowercase(path):
    """SCOWL's lowercase spellings, sizes through 70."""
    assert hashlib.sha256(open(path, "rb").read()).hexdigest() == SCOWL_SHA256
    words = set()
    with tarfile.open(path) as archive:
        for entry in archive:
            m = re.fullmatch(r".*/final/(english|british|american)-(words|contractions|proper-names|upper|abbreviations)\.(\d+)", entry.name)
            if m and int(m[3]) <= 70:
                words.update(w for w in archive.extractfile(entry).read().decode("latin-1").split() if w == w.lower())
    return words


def model_words(path):
    """The model's words with -ln P(word), and its stored pairs (first, second) with -ln P(second | first)."""
    data = open(path, "rb").read()
    version, words, pairs, blob, alphabet = struct.unpack_from("<IIIII", data, 4)
    assert data[:4] == b"HXTM" and version == 3
    letters = {}
    for i in range(alphabet):
        scalar, code, _, _ = struct.unpack_from("<IBBH", data, 28 + 8 * i)
        letters[code] = chr(scalar)
    offsets_at = 28 + 8 * alphabet
    rows_at = offsets_at + 4 * (words + 1) + words
    rows_at += -rows_at % 4
    blob_at = rows_at + 4 * (words + 1) + 4 * pairs + pairs
    assert blob_at + blob == len(data)
    offsets = struct.unpack_from(f"<{words + 1}I", data, offsets_at)
    spelled = ["".join(letters[b] for b in data[blob_at + offsets[i]:blob_at + offsets[i + 1]]) for i in range(words)]
    unigram = {spelled[i]: data[offsets_at + 4 * (words + 1) + i] / 8 for i in range(words)}
    rows = struct.unpack_from(f"<{words + 1}I", data, rows_at)
    seconds_at = rows_at + 4 * (words + 1)
    seconds = struct.unpack_from(f"<{pairs}I", data, seconds_at)
    costs_at = seconds_at + 4 * pairs
    pair = {(spelled[a], spelled[seconds[k]]): data[costs_at + k] / 8 for a in range(words) for k in range(rows[a], rows[a + 1])}
    return unigram, pair


def joined(word, share, unigram, pair):
    """Whether `word` is two model words run together, as scanned books (and typists) do ("Iwant",
    "apartir"): the stored pair is at least JOINED times as common as the word itself (`share` of
    the corpus). Those are no names."""
    best = 0.0
    for k in range(1, len(word)):
        a, b = word[:k], word[k:]
        if (a, b) in pair:
            best = max(best, math.exp(-unigram[a] - pair[(a, b)]))
    return best >= JOINED * share


def count_shard(job):
    lang, index, url, work = job
    out = os.path.join(work, os.path.basename(url) + ".tsv")
    if os.path.exists(out):
        return out
    letters = LETTERS[lang]
    token_re = re.compile(f"^[{letters.upper()}{letters}]?[{letters}]+$".encode("utf-8"))
    download = subprocess.Popen(["curl", "-sSfL", "--http1.1", "--retry", "8", "--retry-all-errors", url], stdout=subprocess.PIPE)
    unzip = subprocess.Popen(["gzip", "-dc"], stdin=download.stdout, stdout=subprocess.PIPE, bufsize=1 << 20)
    download.stdout.close()
    counts = {}
    for line in unzip.stdout:
        tab = line.find(b"\t")
        token = line[:tab]
        if b"_" in token or not token_re.match(token):
            continue
        total = 0
        for field in line[tab + 1:].split(b"\t"):
            year, count, _ = field.split(b",", 2)
            if FIRST_YEAR <= int(year) <= LAST_YEAR:
                total += int(count)
        if total >= 4:
            counts[token] = counts.get(token, 0) + total
    unzip.wait(); download.wait()
    if unzip.returncode or download.returncode:
        raise RuntimeError(f"{url}: curl {download.returncode}, gzip {unzip.returncode}")
    with open(out + ".part", "wb") as f:
        for token, count in counts.items():
            f.write(token + b"\t" + str(count).encode() + b"\n")
    os.replace(out + ".part", out)
    print(f"[{index}] {os.path.basename(url)}: {len(counts):,} tokens", flush=True)
    return out


def main():
    lang, model, work, output = sys.argv[1:5]
    lowercase = scowl_lowercase(sys.argv[5]) if lang == "en" else set()
    os.makedirs(work, exist_ok=True)
    merged = os.path.join(work, "counts.tsv")
    if not os.path.exists(merged):
        code, n = FILES[lang]
        jobs = [(lang, i, f"{BASE}/{code}/1-{i:05d}-of-{n:05d}.gz", work) for i in range(n)]
        with multiprocessing.Pool(4) as pool:
            paths = pool.map(count_shard, jobs, chunksize=1)
        low, cap = {}, {}
        for path in paths:
            for line in open(path, "rb"):
                token, count = line.rstrip(b"\n").split(b"\t")
                word = token.decode("utf-8")
                key = word.lower()
                target = low if word == key else cap
                target[key] = target.get(key, 0) + int(count)
        with open(merged + ".part", "w", encoding="utf-8") as f:
            for key in set(low) | set(cap):
                f.write(f"{key}\t{low.get(key, 0)}\t{cap.get(key, 0)}\n")
        os.replace(merged + ".part", merged)
    unigram, pair = model_words(model)
    words = set(unigram)
    def folded(w):
        return "".join(c for c in unicodedata.normalize("NFD", w) if not unicodedata.combining(c))
    accented_twin = set()
    if lang in ("fr", "it"):
        twins = {}
        for w in words:
            twins.setdefault(folded(w), []).append(w)
        accented_twin = {w for group in twins.values() if len(group) > 1 for w in group
                         if w == folded(w) and any(o != w and unigram[o] < unigram[w] for o in group)}
    corpus = 0
    for line in open(merged, encoding="utf-8"):
        _, lo, ca = line.rstrip("\n").split("\t")
        corpus += int(lo) + int(ca)
    rows = []
    for line in open(merged, encoding="utf-8"):
        key, lo, ca = line.rstrip("\n").split("\t")
        lo, ca = int(lo), int(ca)
        total = lo + ca
        if len(key) < 2 or total < MIN_TOTAL:
            continue
        share = (200 * ca + total) // (2 * total)
        if key in words:
            if share < SOMETIMES:
                continue
            if share < MODEL_ALWAYS[lang]:
                share = min(share, HELD)
        elif (len(key) < 3 or share < NAME_SHARE or total * 1_000_000 < NAME_PER_MILLION * corpus
              or joined(key, total / corpus, unigram, pair)):
            continue
        if key in lowercase or key in accented_twin:
            share = min(share, HELD)
        rows.append((key, share))
    rows.sort(key=lambda r: r[0].encode("utf-8"))
    data = "".join(f"{k}\t{p}\n" for k, p in rows).encode("utf-8")
    open(output, "wb").write(data)
    inside = sum(1 for k, _ in rows if k in words)
    print(f"{len(rows):,} words ({inside:,} in the model, {len(rows) - inside:,} names outside it), "
          f"{len(data):,} bytes; {hashlib.sha256(data).hexdigest()}")


if __name__ == "__main__":
    main()
