#!/usr/bin/env python3
"""Builds the German capitalization sidecar (HEXKeyboard/TypingModel/de.capitals) for the
lowercase-only German typing model (de.hextm), from the same Google Books Ngram v3 German
unigram shards (2020-02-17, CC BY 3.0) that model was counted from.

    python3 scripts/build-german-capitals.py HEXKeyboard/TypingModel/de.hextm \
        HEXKeyboard/TypingModel/de.capitals

For every word in the model it sums the 1980-2019 counts of that exact all-lowercase token
and of the token with only its first letter uppercased (ä, ö and ü included; a word that
begins with ß has no capital spelling, and ß anywhere else stays as it is). Other casings
(ALLCAPS, mixed) and POS-tagged tokens (any with "_") are ignored. A word is written when
the capitalized share rounds to at least 50% and the two counts together reach MIN_TOTAL
(40 occurrences across 40 years of books, below which the share is mostly noise), except
pronouns and determiners that are capitalized for other reasons: polite Sie, and sentence
starts (FUNCTION_WORDS). Each line is `word<TAB>percent`, sorted by word, with a trailing
newline.

The eight shards are streamed through curl and gzip -dc, four at a time, and never stored.
Each shard's counts are cached under .build-german-capitals/ beside this script, so an
interrupted run resumes where it stopped; the cache is deleted when the run finishes.
"""
import hashlib, multiprocessing, os, shutil, struct, subprocess, sys

BASE = "https://storage.googleapis.com/books/ngrams/books/20200217"
FILES = ("ger", 8)          # ger/1-0000N-of-00008.gz, the German unigram shards
FIRST_YEAR, LAST_YEAR = 1980, 2019
MIN_TOTAL = 40              # occurrences (lowercase + capitalized) a word needs
SHARDS = 4                  # shards streamed at once
WORK = os.path.join(os.path.dirname(os.path.abspath(__file__)), ".build-german-capitals")
FUNCTION_WORDS = frozenset("""
    sie ihr ihre ihren ihrem ihrer ihres ihnen du dich dir dein deine deinen deinem deiner
    deines euch euer eure euren eurem eurer eures ich mich mir wir uns er es ihn ihm man
    sich mein meine meinen meinem meiner meines
""".split())


def capitalized(word):
    """The word with only its first letter uppercased, or None (ß has no capital form)."""
    first = word[0].upper()
    return first + word[1:] if len(first) == 1 else None


def model_words(path):
    """The words of an HXTM v3 model, read as build-typing-model.py writes them."""
    data = open(path, "rb").read()
    if data[:4] != b"HXTM":
        raise ValueError(path + ": not an HXTM file")
    version, words, pairs, blob, alphabet = struct.unpack_from("<IIIII", data, 4)
    if version != 3:
        raise ValueError(f"{path}: HXTM version {version}, not 3")
    letters = {}
    for i in range(alphabet):
        scalar, code, _, _ = struct.unpack_from("<IBBH", data, 28 + 8 * i)
        letters[code] = chr(scalar)
    offsets_at = 28 + 8 * alphabet
    rows_at = offsets_at + 4 * (words + 1) + words
    rows_at += -rows_at % 4
    blob_at = rows_at + 4 * (words + 1) + 4 * pairs + pairs
    if blob_at + blob != len(data):
        raise ValueError(path + ": truncated or unexpected HXTM tables")
    offsets = struct.unpack_from(f"<{words + 1}I", data, offsets_at)
    return ["".join(letters[b] for b in data[blob_at + offsets[i]:blob_at + offsets[i + 1]])
            for i in range(words)]


WANTED = frozenset()


def load_wanted(tokens):
    global WANTED
    WANTED = tokens


def count_shard(job):
    """Streams one shard and writes its counts of the wanted tokens, unless cached."""
    index, url, work = job
    name = os.path.basename(url)
    out = os.path.join(work, name + ".tsv")
    if os.path.exists(out):
        print(f"[{index}] {name}: cached", flush=True)
        return out
    download = subprocess.Popen(
        ["curl", "-sSfL", "--http1.1", "--retry", "8", "--retry-all-errors", url],
        stdout=subprocess.PIPE)
    unzip = subprocess.Popen(
        ["gzip", "-dc"], stdin=download.stdout, stdout=subprocess.PIPE, bufsize=1 << 20)
    download.stdout.close()
    wanted = WANTED
    counts = {}
    lines = 0
    try:
        for line in unzip.stdout:
            lines += 1
            tab = line.find(b"\t")
            if tab < 0:
                continue
            token = line[:tab]
            if token not in wanted:
                continue
            total = 0
            for field in line[tab + 1:].split(b"\t"):
                year, count, _ = field.split(b",", 2)
                if FIRST_YEAR <= int(year) <= LAST_YEAR:
                    total += int(count)
            if total:
                counts[token] = counts.get(token, 0) + total
    finally:
        unzip.stdout.close()
        unzip.wait()
        download.wait()
    if unzip.returncode or download.returncode:
        raise RuntimeError(f"{name}: curl {download.returncode}, gzip {unzip.returncode}")
    with open(out + ".part", "wb") as f:
        for token, count in counts.items():
            f.write(token + b"\t" + str(count).encode() + b"\n")
    os.replace(out + ".part", out)
    print(f"[{index}] {name}: {lines:,} lines, {len(counts):,} wanted tokens", flush=True)
    return out


def merge_counts(paths):
    counts = {}
    for path in paths:
        for line in open(path, "rb"):
            token, count = line.rstrip(b"\n").split(b"\t")
            counts[token] = counts.get(token, 0) + int(count)
    return counts


def percent(capitalized_count, total):
    """100 * capitalized_count / total, rounded half up, without float error."""
    return (200 * capitalized_count + total) // (2 * total)


def build(model_path, out_path):
    words = model_words(model_path)
    tokens = set()
    for w in words:
        tokens.add(w.encode("utf-8"))
        cap = capitalized(w)
        if cap:
            tokens.add(cap.encode("utf-8"))
    tokens = frozenset(tokens)
    # Key the cache by the model's word list, so an interrupted run for another model never
    # leaves partial counts this one would trust.
    work = os.path.join(WORK, hashlib.sha256(b"\n".join(sorted(tokens))).hexdigest()[:16])
    os.makedirs(work, exist_ok=True)
    code, count = FILES
    urls = [f"{BASE}/{code}/1-{i:05d}-of-{count:05d}.gz" for i in range(count)]
    with multiprocessing.Pool(SHARDS, initializer=load_wanted, initargs=(tokens,)) as pool:
        parts = pool.map(count_shard, [(i, url, work) for i, url in enumerate(urls)])
    counts = merge_counts(parts)
    rows = []
    for w in words:
        if w in FUNCTION_WORDS:
            continue
        low = counts.get(w.encode("utf-8"), 0)
        cap = capitalized(w)
        cap = counts.get(cap.encode("utf-8"), 0) if cap else 0
        total = low + cap
        if total >= MIN_TOTAL and percent(cap, total) >= 50:
            rows.append((w, percent(cap, total)))
    rows.sort()
    with open(out_path + ".part", "w", encoding="utf-8", newline="\n") as f:
        for w, p in rows:
            f.write(f"{w}\t{p}\n")
    os.replace(out_path + ".part", out_path)
    high = sum(1 for _, p in rows if p >= 95)
    print(f"{out_path}: {len(rows):,} of {len(words):,} words, {high:,} at 95% or more, "
          f"{len(rows) - high:,} at 50-94%, {len(tokens):,} tokens summed")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    try:
        build(*sys.argv[1:])
    except BaseException:
        print(f"leaving {WORK} for a resume", file=sys.stderr)
        raise
    shutil.rmtree(WORK)
