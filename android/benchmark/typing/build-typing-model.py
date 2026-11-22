#!/usr/bin/env python3
"""Builds a Spatial autocorrect model (HEXKeyboard/TypingModel/<lang>.hextm) from Google
Books Ngram v3 (2020-02-17, CC BY 3.0) word and word-pair counts, keeping only words a
word list knows: SCOWL (sizes up to 70) for English, RLA-ES (LibreOffice's es_ES and
es_MX Hunspell dictionaries, used under their MPL 1.1 option) for Spanish. Counts are summed over 1980-2019.

English also takes two things the books can't give it:
- Contractions at the frequency of the words they stand for. Google Books splits "don't"
  into "do" and "n't", so its own count of "don't" is tiny (14.4 nats in the first model,
  as rare as a word ranked 100,000th). Each contraction in CONTRACTIONS counts as often as
  its expansion's pair ("do not") when that is commoner, and the ones SCOWL lists only with
  a capital ("I'm") or not at all are added in lowercase.
- en-extra-words.tsv beside this script: chat and tech words written for Hex ("app", "lol",
  "csv"), each at its tier's cost; a word the books have only gets commoner.
Both join the vocabulary before pairs are counted, so their pairs are counted too.

    python3 -m venv /tmp/venv && /tmp/venv/bin/pip install spylls   # Spanish only
    /tmp/venv/bin/python scripts/build-typing-model.py es <workdir> HEXKeyboard/TypingModel/es.hextm

Needs about 7 GB free in <workdir> while it runs (English downloads ~240 GB of ngrams,
Spanish ~50 GB, each file deleted once counted) and a fast connection; every stage is
cached in <workdir>, so a rerun resumes. Deterministic for the same inputs.

Format v3 (little-endian), memory-mapped by TypingDecoder.swift:
    "HXTM" u32 version=3, u32 words N, u32 pairs B, u32 blob bytes, u32 alphabet size A,
    char[4] language
    A x (u32 Unicode scalar, u8 code, u8 base code, u16 0)   codes 1..A by scalar; base is
        the unaccented letter's code (é -> e) or the letter's own
    u32[N+1] word offsets into the blob (words sorted by code bytes), u8[N] word cost,
    padding to 4, u32[N+1] first pair of each word, u32[B] second words (sorted within
    each first word), u8[B] pair cost, blob (one code byte per letter)
Costs are -ln(p) in eighths of a nat, clamped to 255: P(word), and P(second | first).
"""
import gzip, math, multiprocessing, os, re, struct, subprocess, sys, tarfile, unicodedata, urllib.request

BASE = "https://storage.googleapis.com/books/ngrams/books/20200217"
FILES = {"en": ("eng", 24, 589), "es": ("spa", 3, 73)}
SCOWL = "https://downloads.sourceforge.net/project/wordlist/SCOWL/2020.12.07/scowl-2020.12.07.tar.gz"
RLA_ES = "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/es/"
RLA_ES_LOCALES = ("es_ES", "es_MX")
FIRST_YEAR = 1980
MAX_WORDS = {"en": 90_000, "es": 110_000}
MAX_BYTES = 3_600_000      # the whole model
MIN_PAIR = 2_000           # pairs rarer than this (1980-2019) can't make the cut
LETTERS = {"en": "abcdefghijklmnopqrstuvwxyz'", "es": "abcdefghijklmnopqrstuvwxyzñáéíóúü"}
CONTRACTIONS = {"en": {
    "don't": "do not", "doesn't": "does not", "didn't": "did not", "can't": "can not", "won't": "will not",
    "isn't": "is not", "aren't": "are not", "wasn't": "was not", "weren't": "were not", "haven't": "have not",
    "hasn't": "has not", "hadn't": "had not", "wouldn't": "would not", "couldn't": "could not",
    "shouldn't": "should not", "mustn't": "must not", "needn't": "need not",
    "i'm": "i am", "i'll": "i will", "i've": "i have", "i'd": "i would",
    "it's": "it is", "that's": "that is", "what's": "what is", "where's": "where is", "who's": "who is",
    "there's": "there is", "here's": "here is", "he's": "he is", "she's": "she is", "let's": "let us",
    "you're": "you are", "we're": "we are", "they're": "they are", "you'll": "you will", "we'll": "we will",
    "they'll": "they will", "he'll": "he will", "she'll": "she will", "it'll": "it will",
    "you've": "you have", "we've": "we have", "they've": "they have", "you'd": "you would",
    "we'd": "we would", "they'd": "they would", "he'd": "he would", "she'd": "she would",
}}
EXTRA_WORDS = {"en": "en-extra-words.tsv"}
TIER_COST = {"1": 10.0, "2": 12.0, "3": 13.5}  # nats: about the 2,000th word, then rarer


def urls(lang, n):
    code, ones, twos = FILES[lang]
    count = ones if n == 1 else twos
    return [f"{BASE}/{code}/{n}-{i:05d}-of-{count:05d}.gz" for i in range(count)]


def fetch(url, path):
    if not os.path.exists(path):
        subprocess.run(["curl", "-sSfL", "--http1.1", "--retry", "8", "--retry-all-errors", "-o", path + ".part", url], check=True)
        os.replace(path + ".part", path)


def lines(path):
    proc = subprocess.Popen(["gzip", "-dc", path], stdout=subprocess.PIPE, bufsize=1 << 20)
    yield from proc.stdout
    proc.wait()


def years(fields):
    total = 0
    for f in fields:
        y, c, _ = f.split(b",")
        if int(y) >= FIRST_YEAR:
            total += int(c)
    return total


def lower(token):
    if token.isascii():
        return token.lower()
    return token.decode("utf-8", "replace").lower().encode("utf-8")


def count_unigrams(job):
    lang, url, work = job
    out = os.path.join(work, "uni", os.path.basename(url) + ".tsv")
    if os.path.exists(out):
        return out
    raw = os.path.join(work, "raw", os.path.basename(url))
    fetch(url, raw)
    word = re.compile(("^[" + LETTERS[lang] + "]+$").encode("utf-8"))
    counts = {}
    for line in lines(raw):
        parts = line.rstrip(b"\n").split(b"\t")
        w = lower(parts[0])
        if b"_" in w or not word.match(w):
            continue
        counts[w] = counts.get(w, 0) + years(parts[1:])
    with open(out + ".part", "wb") as f:
        for w, c in counts.items():
            if c:
                f.write(w + b"\t" + str(c).encode() + b"\n")
    os.replace(out + ".part", out)
    os.remove(raw)
    return out


VOCAB = set()


def load_vocab(path):
    global VOCAB
    VOCAB = {line.split("\t", 1)[0].encode("utf-8") for line in open(path, encoding="utf-8")}


def count_pairs(job):
    lang, url, work = job
    out = os.path.join(work, "bi", os.path.basename(url) + ".tsv")
    if os.path.exists(out):
        return out
    raw = os.path.join(work, "raw", os.path.basename(url))
    fetch(url, raw)
    vocab = VOCAB
    counts = {}
    for line in lines(raw):
        tab = line.find(b"\t")
        ngram = line[:tab]
        space = ngram.find(b" ")
        a, b = lower(ngram[:space]), lower(ngram[space + 1:])
        if a in vocab and b in vocab:
            key = a + b" " + b
            counts[key] = counts.get(key, 0) + years(line[tab + 1:].rstrip(b"\n").split(b"\t"))
    with open(out + ".part", "wb") as f:
        for k, c in counts.items():
            if c >= MIN_PAIR // 4:
                f.write(k + b"\t" + str(c).encode() + b"\n")
    os.replace(out + ".part", out)
    os.remove(raw)
    return out


def word_list(lang, work):
    """The words the word list accepts, as a predicate on lowercase strings."""
    if lang == "en":
        path = os.path.join(work, "scowl.tar.gz")
        fetch(SCOWL, path)
        words = set()
        with tarfile.open(path) as t:
            for m in t.getmembers():
                name = os.path.basename(m.name)
                kind, _, size = name.rpartition(".")
                if kind in ("english-words", "american-words", "british-words", "english-contractions") \
                        and size.isdigit() and int(size) <= 70:
                    for w in t.extractfile(m).read().decode("latin-1").split():
                        if w == w.lower():
                            words.add(w)
        return words.__contains__
    from spylls.hunspell import Dictionary
    dictionaries = []
    for locale in RLA_ES_LOCALES:
        for ext in ("dic", "aff"):
            fetch(f"{RLA_ES}{locale}.{ext}", os.path.join(work, f"{locale}.{ext}"))
        dictionaries.append(Dictionary.from_files(os.path.join(work, locale)))
    return lambda w: any(d.lookup(w) for d in dictionaries)


def check_words(args):
    lang, work, words = args
    accept = word_list(lang, work)
    return [w for w in words if accept(w)]


def cost(p):
    return max(0, min(255, round(-math.log(max(p, 1e-30)) * 8)))


def extra_words(lang, total):
    """EXTRA_WORDS' words with the count their tier's cost stands for."""
    if lang not in EXTRA_WORDS:
        return {}
    words = {}
    for line in open(os.path.join(os.path.dirname(os.path.abspath(__file__)), EXTRA_WORDS[lang]), encoding="utf-8"):
        if not line.strip() or line.startswith("#"):
            continue
        word, tier = line.rstrip("\n").split("\t")
        assert re.fullmatch("[" + LETTERS[lang] + "]+", word), word
        words[word] = max(words.get(word, 0), round(total * math.exp(-TIER_COST[tier])))
    return words


def main(lang, work, out_path):
    for d in ("raw", "uni", "bi"):
        os.makedirs(os.path.join(work, d), exist_ok=True)
    workers = int(os.environ.get("WORKERS", "12"))
    with multiprocessing.Pool(workers) as pool:
        uni_files = pool.map(count_unigrams, [(lang, u, work) for u in urls(lang, 1)])
    counts = {}
    for p in uni_files:
        for line in open(p, "rb"):
            w, c = line.rstrip(b"\n").split(b"\t")
            counts[w] = counts.get(w, 0) + int(c)
    total = sum(counts.values())
    vocab_path = os.path.join(work, "vocab.tsv")
    if not os.path.exists(vocab_path):
        ranked = sorted(counts, key=lambda w: (-counts[w], w))[: MAX_WORDS[lang] * 4]
        texts = [w.decode("utf-8") for w in ranked]
        word_list(lang, work)  # downloads the list once, before the workers read it
        chunks = [texts[i::workers] for i in range(workers)]
        with multiprocessing.Pool(workers) as pool:
            ok = set().union(*map(set, pool.map(check_words, [(lang, work, c) for c in chunks])))
        kept = [w for w in texts if w in ok][: MAX_WORDS[lang]]
        with open(vocab_path, "w", encoding="utf-8") as f:
            for w in kept:
                f.write(f"{w}\t{counts[w.encode('utf-8')]}\n")
    vocab = {}
    for line in open(vocab_path, encoding="utf-8"):
        w, c = line.rstrip("\n").split("\t")
        vocab[w] = int(c)
    for w, c in extra_words(lang, total).items():
        vocab[w] = max(vocab.get(w, 0), counts.get(w.encode("utf-8"), 0), c)
    for w in CONTRACTIONS.get(lang, {}):
        vocab.setdefault(w, max(1, counts.get(w.encode("utf-8"), 0)))
    # Pairs are counted over the vocabulary with these words (a cached bi/ from a run
    # without them lacks their pairs).
    pair_vocab = os.path.join(work, "vocab-pairs.tsv")
    with open(pair_vocab, "w", encoding="utf-8") as f:
        for w in sorted(vocab):
            f.write(f"{w}\t{vocab[w]}\n")

    with multiprocessing.Pool(workers, initializer=load_vocab, initargs=(pair_vocab,)) as pool:
        bi_files = pool.map(count_pairs, [(lang, u, work) for u in urls(lang, 2)])
    pairs = {}
    for p in bi_files:
        for line in open(p, "rb"):
            k, c = line.rstrip(b"\n").split(b"\t")
            pairs[k] = pairs.get(k, 0) + int(c)
    # Pairs of words no longer in the vocabulary (a bi/ cached by a run with more words) go.
    pairs = {k: c for k, c in pairs.items() if all(w.decode("utf-8") in vocab for w in k.split(b" "))}
    for w, expansion in CONTRACTIONS.get(lang, {}).items():
        vocab[w] = max(vocab[w], pairs.get(expansion.encode("utf-8"), 0))
    write_model(lang, vocab, total, pairs, out_path)


def write_model(lang, vocab, total, pairs, out_path):
    scalars = sorted({c for w in vocab for c in w})
    code = {c: i + 1 for i, c in enumerate(scalars)}
    def base(c):
        b = unicodedata.normalize("NFD", c)[0]
        return code.get(b, code[c])
    enc = {w: bytes(code[c] for c in w) for w in vocab}
    words = sorted(vocab, key=lambda w: enc[w])
    index = {w: i for i, w in enumerate(words)}
    blob = b"".join(enc[w] for w in words)
    fixed = 28 + 8 * len(scalars) + 4 * (len(words) + 1) + len(words) + 3 + 4 * (len(words) + 1) + len(blob)
    budget = (MAX_BYTES - fixed) // 5

    # Keep the pairs that tell the most over the single-word model (backoff 0.4):
    # count x log(P(b | a) / (0.4 P(b))), positive only, best first, as many as fit.
    scored = []
    for k, c in pairs.items():
        if c < MIN_PAIR:
            continue
        a, b = k.decode("utf-8").split(" ")
        p = c / vocab[a]
        gain = c * math.log(p / (0.4 * vocab[b] / total))
        if gain > 0:
            scored.append((gain, index[a], index[b], min(p, 1.0)))
    scored.sort(reverse=True)
    kept = sorted((a, b, p) for _, a, b, p in scored[:budget])
    rows = [0] * (len(words) + 1)
    for a, _, _ in kept:
        rows[a + 1] += 1
    for i in range(len(words)):
        rows[i + 1] += rows[i]

    offsets, at = [], 0
    for w in words:
        offsets.append(at)
        at += len(enc[w])
    offsets.append(at)
    out = bytearray(b"HXTM")
    out += struct.pack("<IIIII", 3, len(words), len(kept), len(blob), len(scalars))
    out += lang.encode("ascii").ljust(4, b"\0")
    for c in scalars:
        out += struct.pack("<IBBH", ord(c), code[c], base(c), 0)
    out += struct.pack(f"<{len(offsets)}I", *offsets)
    out += bytes(cost(vocab[w] / total) for w in words)
    out += b"\0" * (-len(out) % 4)
    out += struct.pack(f"<{len(rows)}I", *rows)
    out += struct.pack(f"<{len(kept)}I", *(b for _, b, _ in kept))
    out += bytes(cost(p) for _, _, p in kept)
    out += blob
    with open(out_path, "wb") as f:
        f.write(out)
    print(f"{out_path}: {lang}, {len(words)} words, {len(kept)} pairs of {len(scored)} useful, "
          f"{len(scalars)} letters, {len(out) / 1e6:.2f} MB")


if __name__ == "__main__":
    if len(sys.argv) != 4 or sys.argv[1] not in FILES:
        sys.exit(__doc__)
    main(*sys.argv[1:])
