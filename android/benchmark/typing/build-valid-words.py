#!/usr/bin/env python3
"""Validity lists (<lang>.valid): spellings outside a keyboard model that are still real words, so the
decoder never splits them into words (iOS asks its spelling dictionary, UITextChecker). One lowercase
word per line, sorted by UTF-8 bytes, with a trailing newline; only words the model lacks. English's
lines are the word alone; the others add a tab and the word's cost in eighths of a nat,
round(8 ln(corpus / count)), as the model stores its words', so a rare word costs what it is.

    python3 build-valid-words.py en scowl-2020.12.07.tar.gz en.hextm en.valid
    python3 build-valid-words.py {es|de|fr|it} MODEL.hextm WORKDIR OUTPUT.valid [MIN_COUNT]
    python3 build-valid-words.py nb MODEL.hextm WORKDIR OUTPUT.valid [MIN_COUNT]

English: SCOWL spellings, sizes through 70. Spanish, German, French and Italian: Google Books Ngram
v3 1-grams (2020-02-17, CC BY 3.0), 1980-2019, as build-capitals.py counts them into
WORKDIR/counts.tsv: every spelling of two letters or more the books use at least MIN_COUNT times,
in lowercase or with a capital. Left out: two model words run together whose pair is ten times as
common ("apartir", scanned books join them as typists do), in French and Italian an elision
without its apostrophe that the model has with it ("cest"), so space still puts it back, and in every
language a spelling that is a model word with an accent changed or left out ("volvere", "brulant"),
which books misspell as typists do.
Bokmål: Norsk ordbank's standard full forms (2022 release, CC BY 4.0) that the NB N-gram 2026
books (language nob, CC0) use at least MIN_COUNT times, 1980-2019.
"""
import collections, csv, hashlib, importlib, io, math, os, re, sys, tarfile, unicodedata
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
capitals = importlib.import_module("build-capitals")

SCOWL_SHA256 = "5587667caa20c4891390c2d42dbb4d5c4c3f41bee77af1457ece3ba23fb859cc"
# The fewest uses that make a spelling a word, chosen per language on the typing benchmark
# against both the rare words it keeps and the typos it lets stand (README.md).
MIN_COUNT = {"es": 3000, "de": 10000, "fr": 3000, "it": 3000, "nb": 1000}
ELISIONS = {
    "fr": ["c", "d", "j", "l", "m", "n", "s", "t", "qu", "jusqu", "lorsqu", "puisqu", "quoiqu", "presqu"],
    "it": ["c", "d", "l", "m", "n", "s", "t", "v", "un", "all", "dall", "dell", "nell", "sull", "coll",
           "quest", "quell", "bell", "sant", "tutt", "nessun", "buon"],
}


def write(output, words):
    """`words`: a set of spellings, or a dict of spelling to cost."""
    line = (lambda w: f"{w}\t{words[w]}\n") if isinstance(words, dict) else (lambda w: w + "\n")
    data = "".join(line(w) for w in sorted(words, key=lambda w: w.encode("utf-8"))).encode("utf-8")
    Path(output).write_bytes(data)
    print(f"{len(words):,} spellings outside the model, {len(data):,} bytes; {hashlib.sha256(data).hexdigest()}")


def folded(w):
    return "".join(c for c in unicodedata.normalize("NFD", w) if not unicodedata.combining(c))


def cost(count, corpus):
    return min(255, round(8 * math.log(corpus / count)))


def english(source, model, output):
    assert hashlib.sha256(Path(source).read_bytes()).hexdigest() == SCOWL_SHA256
    scored = set(capitals.model_words(model)[0])
    valid = set()
    with tarfile.open(source) as archive:
        for entry in archive:
            m = re.fullmatch(r".*/final/(english|british|american)-(words|contractions|proper-names|upper|abbreviations)\.(\d+)", entry.name)
            if m and int(m[3]) <= 70:
                for w in archive.extractfile(entry).read().decode("latin-1").lower().split():
                    if re.fullmatch("[a-z]{2,32}", w) and w not in scored:
                        valid.add(w)
    write(output, valid)


def books(lang, model, work, output, least):
    unigram, pair = capitals.model_words(model)
    merged = capitals.merged_counts(lang, work)
    letters = re.compile(f"[{capitals.LETTERS[lang]}]{{2,32}}").fullmatch
    rows, corpus = [], 0
    for line in open(merged, encoding="utf-8"):
        key, lo, ca = line.rstrip("\n").split("\t")
        total = int(lo) + int(ca)
        corpus += total
        if total >= least and key not in unigram and letters(key):
            rows.append((key, total))
    def elided(word, share):
        return any(word.startswith(p) and len(word) > len(p) and p + "'" + word[len(p):] in unigram
                   and math.exp(-unigram[p + "'" + word[len(p):]]) >= 10 * share for p in ELISIONS.get(lang, []))
    twins = {folded(w) for w in unigram}
    valid = {k: cost(n, corpus) for k, n in rows if not capitals.joined(k, n / corpus, unigram, pair)
             and not elided(k, n / corpus) and folded(k) not in twins}
    print(f"{lang}: {len(rows) - len(valid):,} run-together or misaccented spellings left out", flush=True)
    write(output, valid)


def norwegian(model, work, output, least):
    layout = importlib.import_module("build-layout-model")
    work = Path(work)
    work.mkdir(parents=True, exist_ok=True)
    bank = work / "ordbank-2022.tar.gz"
    layout.builder.fetch(layout.ORDBANK, str(bank))
    if hashlib.sha256(bank.read_bytes()).hexdigest() != "57d91cb3f17b85befa50a3b56fcaed9800ca665b39486fe97b668add914d5f60":
        raise ValueError("Unexpected Norsk ordbank 2022 release")
    with tarfile.open(bank, "r:gz") as archive, archive.extractfile("fullformsliste.txt") as entry:
        rows = csv.DictReader(io.TextIOWrapper(entry, encoding="latin-1"), delimiter="\t", quoting=csv.QUOTE_NONE)
        forms = {layout.normalize(r["OPPSLAG"]) for r in rows if r["NORMERING"] == "normert" and r["TILDATO"] == "4000"}
    counts = work / "counts.tsv"
    if not counts.exists():
        path = work / "digibok-unigram.jsonl.zst"
        layout.download_nb(layout.NB + "unigram.jsonl.zst", path, 1_269_473_951)
        uni = layout.nb_counts(path, False, layout.WORD.fullmatch)
        counts.with_suffix(".part").write_text("".join(f"{w}\t{n}\n" for w, n in uni.items()), encoding="utf-8")
        counts.with_suffix(".part").replace(counts)
    scored = set(capitals.model_words(model)[0])
    twins = {folded(w) for w in scored}
    rows = [line.rstrip("\n").split("\t") for line in open(counts, encoding="utf-8")]
    corpus = sum(int(n) for _, n in rows)
    valid = {w: cost(int(n), corpus) for w, n in rows
             if int(n) >= least and len(w) >= 2 and w in forms and w not in scored and folded(w) not in twins}
    write(output, valid)


if __name__ == "__main__":
    lang = sys.argv[1] if len(sys.argv) > 1 else ""
    if lang == "en" and len(sys.argv) == 5:
        english(*sys.argv[2:5])
    elif lang in ("es", "de", "fr", "it", "nb") and len(sys.argv) in (5, 6):
        least = int(sys.argv[5]) if len(sys.argv) == 6 else MIN_COUNT[lang]
        if lang == "nb":
            norwegian(sys.argv[2], sys.argv[3], sys.argv[4], least)
        else:
            books(lang, sys.argv[2], sys.argv[3], sys.argv[4], least)
    else:
        sys.exit(__doc__)
