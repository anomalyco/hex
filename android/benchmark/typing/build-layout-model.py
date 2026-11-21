#!/usr/bin/env python3
"""Build German and Bokmål HXTM v3 files with the shared iOS count reader / model writer.
    python3 build-layout-model.py {de|nb} SOURCE_CACHE OUTPUT.hextm
German: every untagged German Google Books v3 shard, 1980–2019. Bokmål: NB N-gram
2026 digibok, language nob, same years, filtered by Norsk ordbank full forms.
"""
import collections
import concurrent.futures
import csv
import importlib
import hashlib
import http.client
import io
import json
import re
import subprocess
import sys
import unicodedata
import urllib.request
import shutil
import time
import tarfile
from pathlib import Path

builder = importlib.import_module("build-typing-model")
builder.FILES["de"] = ("ger", 8, 181)
builder.LETTERS["de"] = "abcdefghijklmnopqrstuvwxyzäöüß"
builder.MAX_WORDS["de"] = 95_000
# German uses only the corpus, without importing another spelling dictionary.
# The iOS reader lowercases before adding counts; keep the same alphabet rule
# on Unicode scalars before the shared writer assigns their compact codes.
def german_words(lang, work):
    return re.compile(r"[a-zäöüß]{1,32}\Z").fullmatch
builder.word_list = german_words

NB = "https://www.nb.no/sbfil/ngram/ngram_2026/digibok-"
ORDBANK = "https://www.nb.no/sbfil/leksikalske_databaser/ordbank/20220201_norsk_ordbank_nob_2005.tar.gz"
WORD = re.compile(r"[a-zæøåéèêóôüäö]{1,32}\Z")

def normalize(s):
    return unicodedata.normalize("NFC", s).lower()

def nb_counts(path, is_pair, accept):
    counts = collections.Counter()
    with subprocess.Popen(["zstd", "-dc", path], stdout=subprocess.PIPE) as process:
        for line in process.stdout:
            if b'"nob"' not in line:
                continue
            row = json.loads(line)
            if row["lang"] != "nob":
                continue
            first = normalize(row["first"])
            if not accept(first):
                continue
            if is_pair:
                second = normalize(row["second"])
                if not accept(second):
                    continue
                key = (first + " " + second).encode("utf-8")
            else:
                key = first
            n = sum(v for y, v in row["counts"].items() if "1980" <= y <= "2019")
            if n:
                counts[key] += n
        if process.wait():
            raise ValueError("Incomplete zstd stream: " + str(path))
    return counts

def download_nb(url, path, size):
    # The NB server may close long transfers. Resume independently verified byte ranges.
    if path.is_file() and path.stat().st_size == size:
        return
    parts = path.with_suffix(".ranges")
    parts.mkdir(exist_ok=True)
    block = 4 << 20
    def read(i):
        start, end = i * block, min((i+1)*block, size)
        part = parts / str(i)
        for attempt in range(9):
            pos = start + (part.stat().st_size if part.exists() else 0)
            if pos == end:
                return part
            if pos > end:
                raise ValueError("Oversized NB range")
            req = urllib.request.Request(url, headers={"Range": f"bytes={pos}-{end-1}", "Accept-Encoding": "identity"})
            try:
                with urllib.request.urlopen(req, timeout=60) as res:
                    if res.status != 206 or res.headers.get("Content-Range") != f"bytes {pos}-{end-1}/{size}":
                        raise ValueError("NB returned an unexpected range")
                    with part.open("ab") as f:
                        shutil.copyfileobj(res, f, length=262144)
                if part.stat().st_size == end-start:
                    return part
            except (OSError, http.client.IncompleteRead, EOFError) as e:
                if attempt == 8:
                    raise RuntimeError(f"Could not fetch {url}, range {i}") from e
                time.sleep(min(2**attempt, 10))
        raise IOError("Incomplete NB range " + str(i))
    with concurrent.futures.ThreadPoolExecutor(max_workers=16) as pool:
        complete = list(pool.map(read, range((size + block - 1)//block)))
    assembled = path.with_suffix(".assembling")
    with assembled.open("wb") as out:
        for part in complete:
            with part.open("rb") as f:
                shutil.copyfileobj(f, out)
    if assembled.stat().st_size != size:
        raise IOError("Incomplete NB archive")
    assembled.replace(path)
    shutil.rmtree(parts)

def norwegian(work, out):
    work.mkdir(parents=True, exist_ok=True)
    bank = work / "ordbank-2022.tar.gz"
    builder.fetch(ORDBANK, str(bank))
    if hashlib.sha256(bank.read_bytes()).hexdigest() != "57d91cb3f17b85befa50a3b56fcaed9800ca665b39486fe97b668add914d5f60":
        raise ValueError("Unexpected Norsk ordbank 2022 release")
    # The 2026 inflection-only TSV omits uninflected words such as på and og.
    with tarfile.open(bank, "r:gz") as archive, archive.extractfile("fullformsliste.txt") as entry:
        rows = csv.DictReader(io.TextIOWrapper(entry, encoding="latin-1"), delimiter="\t", quoting=csv.QUOTE_NONE)
        forms = {normalize(r["OPPSLAG"]) for r in rows if r["NORMERING"] == "normert" and r["TILDATO"] == "4000"}
    paths = [work / ("digibok-" + n + ".jsonl.zst") for n in ("unigram", "bigram")]
    archives = [(1_269_473_951, "b430ef4e19caf1a12bcf47e1ee4f6866f27943ae133e141c57d1a3db4dd8f0c9"),
                (3_832_769_124, "b6d2f3ca5492288d5f1c66fc0a3976a7239502cb690f77d493a1cdd97c5ebe1e")]
    for path, (size, expected) in zip(paths, archives):
        download_nb(NB + path.name.removeprefix("digibok-"), path, size)
        digest = hashlib.sha256()
        with path.open("rb") as src:
            for block in iter(lambda: src.read(1 << 20), b""):
                digest.update(block)
        if digest.hexdigest() != expected:
            raise ValueError("Unexpected NB archive: " + str(path))
    uni = nb_counts(paths[0], False, WORD.fullmatch)
    total = sum(uni.values())
    vocab = dict(sorted(((w, n) for w, n in uni.items() if w in forms),
                        key=lambda item: (-item[1], item[0]))[:95_000])
    pairs = nb_counts(paths[1], True, vocab.__contains__)
    print(f"Bokmål: {total:,} word occurrences; {len(forms):,} Ordbank forms; {len(pairs):,} filtered pairs", flush=True)
    builder.write_model("nb", vocab, total, pairs, str(out))

if __name__ == "__main__":
    if len(sys.argv) != 4 or sys.argv[1] not in ("de", "nb"):
        sys.exit(__doc__)
    lang, work, out = sys.argv[1:]
    if lang == "de":
        builder.main(lang, work, out)
    else:
        norwegian(Path(work), Path(out))
