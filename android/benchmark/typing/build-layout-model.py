#!/usr/bin/env python3
"""Build German, French, Italian, Bokmål and Portuguese HXTM v3 files with the shared iOS count reader / model writer.
    python3 build-layout-model.py {de|fr|it|nb|pt} SOURCE_CACHE OUTPUT.hextm
German, French and Italian: every untagged Google Books v3 shard of the language, 1980–2019,
spellings from the corpus alone (no second dictionary). French and Italian keep elided forms
as the books' tokens have them ("c'est", "l'homme", "dell'anno"), the apostrophe only between
letters. Bokmål: NB N-gram 2026 digibok, language nob, same years, filtered by Norsk ordbank
full forms. Portuguese (Brazil): the Leipzig Corpora Collection's news downloads, each sentence
given to Brazil or Portugal by its site, and ParlaIbero's Brazilian plenary record, the files
pt-inputs.tsv pins; spellings from the Brazilian counts alone, less those the 1990 agreement retired
and plain spellings of a much commoner marked word, plus pt-extra-words.tsv.
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
builder.FILES["fr"] = ("fre", 6, 115)
builder.LETTERS["fr"] = "abcdefghijklmnopqrstuvwxyzàâæçéèêëîïôœùûüÿ'"
builder.MAX_WORDS["fr"] = 100_000
builder.FILES["it"] = ("ita", 2, 60)
builder.LETTERS["it"] = "abcdefghijklmnopqrstuvwxyzàèéìíîòóùú'"
builder.MAX_WORDS["it"] = 100_000
# German, French and Italian use only the corpus, without importing another spelling
# dictionary. The iOS reader lowercases before adding counts; keep the same alphabet rule
# on Unicode scalars before the shared writer assigns their compact codes. An apostrophe
# stands only between letters (an elision), never alone or at a word's edge.
CORPUS_WORDS = {
    "de": r"[a-zäöüß]{1,32}\Z",
    "fr": r"(?=.{1,32}\Z)[a-zàâæçéèêëîïôœùûüÿ]+(?:'[a-zàâæçéèêëîïôœùûüÿ]+)?\Z",
    "it": r"(?=.{1,32}\Z)[a-zàèéìíîòóùú]+(?:'[a-zàèéìíîòóùú]+)?\Z",
}
def corpus_words(lang, work):
    return re.compile(CORPUS_WORDS[lang]).fullmatch
builder.word_list = corpus_words

# The articles and pronouns French and Italian elide before a vowel ("c'est", "dell'anno").
ELISIONS = {
    "fr": ["c", "d", "j", "l", "m", "n", "s", "t", "qu", "jusqu", "lorsqu", "puisqu", "quoiqu", "presqu"],
    "it": ["c", "d", "l", "m", "n", "s", "t", "v", "un", "all", "dall", "dell", "nell", "sull", "coll",
           "quest", "quell", "bell", "sant", "tutt", "nessun", "buon"],
}

def run_together(word, vocab, elisions):
    """Whether `word` is an elision typed without its apostrophe ("cest", "quil", "dellanno"): the
    same letters with the apostrophe are a model word at least ten times as common. Scanned books
    and typists both drop it; as a word of its own it would keep the keyboard from putting it back."""
    if "'" in word:
        return False
    return any(word.startswith(p) and len(word) > len(p) and vocab.get(p + "'" + word[len(p):], 0) >= 10 * vocab[word]
               for p in elisions)

def refine(lang, work, out):
    """Rewrites the model without run-together elisions, from the counts the shared builder cached."""
    work = Path(work)
    total = 0
    for path in sorted((work / "uni").glob("*.tsv")):
        for line in open(path, "rb"):
            total += int(line.rstrip(b"\n").split(b"\t")[1])
    vocab = {}
    for line in open(work / "vocab.tsv", encoding="utf-8"):
        w, c = line.rstrip("\n").split("\t")
        vocab[w] = int(c)
    dropped = sorted(w for w in vocab if run_together(w, vocab, ELISIONS[lang]))
    for w in dropped:
        del vocab[w]
    pairs = {}
    for path in sorted((work / "bi").glob("*.tsv")):
        for line in open(path, "rb"):
            k, c = line.rstrip(b"\n").split(b"\t")
            a, b = k.decode("utf-8").split(" ")
            if a in vocab and b in vocab:
                pairs[k] = pairs.get(k, 0) + int(c)
    print(f"{lang}: {len(dropped)} run-together elisions left out: {' '.join(dropped[:40])}", flush=True)
    builder.write_model(lang, vocab, total, pairs, str(out))

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

# Portuguese (Brazil). Google Books has no Portuguese. Words and word pairs come from the Leipzig
# Corpora Collection's downloadable news corpora (CC BY 4.0; its website and API are CC BY-NC and are
# not used), each sentence given to Brazil or Portugal by the site it came from, and from ParlaIbero's
# plenary records of Brazil's Câmara dos Deputados and Portugal's Assembleia da República (CC BY 4.0).
# pt-inputs.tsv beside this script pins every file: its URL, fetch date, size and SHA-256.
PT_INPUTS = Path(__file__).resolve().with_name("pt-inputs.tsv")
PT_LETTERS = "abcdefghijklmnopqrstuvwxyzáàâãçéêíóôõú"
builder.LETTERS["pt"] = PT_LETTERS
builder.MAX_WORDS["pt"] = 100_000
builder.EXTRA_WORDS["pt"] = "pt-extra-words.tsv"
PT_WORD = re.compile(f"[{PT_LETTERS}]+")
PT_TOKEN = re.compile(r"\w+|[^\w\s]")
# The only one-letter words; any other letter alone is an initial or an abbreviation.
PT_ONE_LETTER = {"a", "à", "e", "é", "o", "ó"}
# The fewest times a pair is seen to be kept: the corpus is a few hundred million words, the books'
# hundreds of billions.
PT_MIN_PAIR = 5
# ParlaIbero's years: Brazil's from 2009, when the 1990 spelling agreement came into force there;
# Portugal's from 1995, before which its record is OCR of print.
PT_YEARS = {"br": ("2009", "2025"), "pt": ("1995", "2025")}
# Each news count counts this many times a parliament's: a keyboard's words are closer to the news
# than to "Sr. Presidente" and "nobre Deputado". Which word follows which comes from the news alone:
# every speech turn opens with the same formulas, so "obrigado" was followed by "sr" and "deputado".
PT_WEIGHT = {"leipzig": 10, "parlaibero": 1}
# Spellings only one country writes, which decide a news site's country when its domain doesn't.
PT_MARKERS = {
    "br": re.compile(r"\b(?:econômic|gênero|ônibus|contato|recepção|registro|usuári|equipe\b|esporte|antônio|"
                     r"acadêmic|fenômeno|prêmio|patrimônio|cerimônia|quilômetro|bebê)"),
    "pt": re.compile(r"\b(?:económic|género|autocarro|facto\b|contacto|receção|registo\b|comboio|telemóvel|ecrã|"
                     r"utilizador|ficheiro|equipa\b|desporto|antónio|académic|fenómeno|prémio|património|cerimónia|"
                     r"quilómetro|bebé|acção|actual|director|projecto|óptim)"),
}
# Spellings the 1990 agreement retired in Brazil (Portugal never wrote them): an acute on a stressed
# éi or ói before a final syllable ("idéia", "heróico"), ô in "ôo" ("vôo"), ê in "êem" ("vêem"), and
# the accents that only told words apart ("pára", "pêlo"). Each is left out where the new spelling
# is counted too.
# The words ContextWords.kt's Portuguese groups choose between by the words around them: a verb the
# plain-twin rule would leave out ("duvida" beside "dúvida") stays, so the context can pick it.
PT_CONTEXT_WORDS = set("""e é esta está estas estás nos nós a à as às la lá da dá de dê va vá ai aí pais país por pôr
    pode pôde tem têm vem vêm mas más que quê sabia sábia secretaria secretária duvida dúvida duvidas dúvidas ira irá
    publica pública publico público pratica prática praticas práticas pratico prático critica crítica criticas críticas
    analise análise divida dívida calculo cálculo media média formula fórmula anuncio anúncio noticia notícia
    fabrica fábrica valida válida numero número inicio início estimulo estímulo peca peça copia cópia magoa mágoa
    negocio negócio""".split())
PT_RETIRED = re.compile(r".*[éó]i(?:as?|os?|cas?|cos?)|.*ôos?|.*êem|pára|pêlos?|pólos?|pêras?")


def pt_inputs(work):
    """Every pinned input in WORK/inputs, fetched once and checked against pt-inputs.tsv."""
    rows = [line.split("\t") for line in PT_INPUTS.read_text(encoding="utf-8").splitlines()
            if line and not line.startswith("#")]
    folder = Path(work) / "inputs"
    folder.mkdir(parents=True, exist_ok=True)
    paths = {}
    for name, url, _, size, digest, _ in rows:
        path = folder / name
        if not path.exists():
            subprocess.run(["curl", "-sSfL", "-A", "Mozilla/5.0", "--retry", "8", "--retry-all-errors",
                            "-o", str(path) + ".part", url], check=True)
            Path(str(path) + ".part").replace(path)
        h = hashlib.sha256()
        with path.open("rb") as f:
            for block in iter(lambda: f.read(1 << 20), b""):
                h.update(block)
        if path.stat().st_size != int(size) or h.hexdigest() != digest:
            raise ValueError(f"{path}: {path.stat().st_size} bytes, SHA-256 {h.hexdigest()}; pt-inputs.tsv says {size}, {digest}")
        paths[name] = path
    return paths


class PtCounts:
    """Words (lowercase: all uses, then lowercase and capitalised uses, a sentence's first word left
    out of those two) and word pairs, one counter per country. A pair may span a comma or a hyphen,
    which people typing leave out or type as a key of its own ("sexta-feira"), never other
    punctuation or a word with other characters. Pairs spill to sorted runs on disk."""
    SPILL = 6_000_000

    def __init__(self, work, name):
        self.work, self.name, self.tokens = Path(work), name, collections.Counter()
        self.uni = {"br": {}, "pt": {}}
        self.pairs = {"br": {}, "pt": {}}
        self.runs = 0

    def sentence(self, region, text):
        uni, pairs = self.uni[region], self.pairs[region]
        prev, first, n = None, True, 0
        for t in PT_TOKEN.findall(text):
            if t == "," or t == "-":
                continue
            key = t.lower()
            if PT_WORD.fullmatch(key):
                n += 1
                e = uni.get(key)
                if e is None:
                    e = uni[key] = [0, 0, 0]
                e[0] += 1
                if not first:
                    if t == key:
                        e[1] += 1
                    elif t[0].isupper() and t[1:] == key[1:]:
                        e[2] += 1
                if prev is not None:
                    k = prev + " " + key
                    pairs[k] = pairs.get(k, 0) + 1
                prev = key
            else:
                prev = None
            if t[0].isalnum():
                first = False
        self.tokens[region] += n
        if len(pairs) > self.SPILL:
            self.spill(region)

    def spill(self, region):
        pairs = self.pairs[region]
        if not pairs:
            return
        path = self.work / "runs" / f"{self.name}-{region}-{self.runs:04d}.tsv"
        self.runs += 1
        with path.open("w", encoding="utf-8") as f:
            for k in sorted(pairs):
                f.write(f"{k}\t{pairs[k]}\n")
        pairs.clear()

    def close(self):
        for region in ("br", "pt"):
            self.spill(region)
            with (self.work / "runs" / f"{self.name}-{region}.uni").open("w", encoding="utf-8") as f:
                for k, (total, low, cap) in self.uni[region].items():
                    f.write(f"{k}\t{total}\t{low}\t{cap}\n")
        return self.name, dict(self.tokens)


def pt_leipzig_extract(job):
    """One Leipzig archive as WORK/leipzig/NAME.tsv (each sentence's site and text), with the
    country markers per site and a stable hash per sentence, to count a sentence two archives share
    once."""
    from urllib.parse import urlsplit
    path, work = job
    out = Path(work) / "leipzig" / (path.name[:-len(".tar.gz")] + ".tsv")
    sources, sentences, links = {}, [], {}
    with tarfile.open(path, "r:gz") as t:
        for m in t:
            kind = m.name.rsplit("-", 1)[-1]
            if kind not in ("sources.txt", "sentences.txt", "inv_so.txt"):
                continue
            for line in io.TextIOWrapper(t.extractfile(m), encoding="utf-8", errors="replace"):
                p = line.rstrip("\n").split("\t")
                if kind == "sources.txt" and len(p) >= 2:
                    sources[p[0]] = (urlsplit(p[1]).hostname or "").lower()
                elif kind == "sentences.txt" and len(p) >= 2:
                    sentences.append((p[0], unicodedata.normalize("NFC", p[1])))
                elif kind == "inv_so.txt" and len(p) >= 2:
                    links.setdefault(p[1], p[0])  # source, sentence
    votes, hashes = collections.defaultdict(lambda: [0, 0]), []
    with out.open("w", encoding="utf-8") as f:
        for sid, text in sentences:
            host = sources.get(links.get(sid, ""), "")
            f.write(f"{host}\t{text}\n")
            low = text.lower()
            for i, region in enumerate(("br", "pt")):
                if PT_MARKERS[region].search(low):
                    votes[host][i] += 1
            hashes.append(int.from_bytes(hashlib.blake2b(text.encode("utf-8"), digest_size=8).digest(), "little"))
    return path.name, dict(votes), hashes


def pt_region(host, sites):
    if host.endswith(".br"):
        return "br"
    if host.endswith(".pt"):
        return "pt"
    return sites.get(host)


def pt_leipzig_count(job):
    name, work, sites, repeats = job
    counts = PtCounts(work, "leipzig-" + name[:-len(".tar.gz")])
    with (Path(work) / "leipzig" / (name[:-len(".tar.gz")] + ".tsv")).open(encoding="utf-8") as f:
        for i, line in enumerate(f):
            if i in repeats:
                continue
            host, text = line.rstrip("\n").split("\t", 1)
            region = pt_region(host, sites)
            if region:
                counts.sentence(region, text)
    return counts.close()


def pt_parlaibero_count(job):
    """A ParlaIbero CSV's speech (dm_speech 1) in PT_YEARS, without the record's stage directions
    ("(Risos.)", "(Palmas.)"), one sentence at a time."""
    path, region, work = job
    csv.field_size_limit(1 << 30)
    first, last = PT_YEARS[region]
    counts = PtCounts(work, "parlaibero-" + region)
    stage = re.compile(r"\([^()]{0,60}\)")
    split = re.compile(r"(?<=[.!?…])\s+")
    with open(path, encoding="utf-8", newline="") as f:
        for row in csv.DictReader(f):
            if row["dm_speech"] != "1" or not first <= row["date"][:4] <= last:
                continue
            text = stage.sub(" ", unicodedata.normalize("NFC", row["text"]))
            for s in split.split(text):
                counts.sentence(region, s)
    return counts.close()


def pt_merge(work, region, group):
    """WORK/REGION-GROUP.uni.tsv (word, all uses, lowercase uses, capitalised uses) and
    REGION-GROUP.bi.tsv (pairs seen at least twice), summed over the group's runs (leipzig or
    parlaibero)."""
    import heapq
    runs = Path(work) / "runs"
    uni = {}
    for path in sorted(runs.glob(f"{group}-*-{region}.uni")):
        for line in path.open(encoding="utf-8"):
            k, *n = line.rstrip("\n").split("\t")
            e = uni.setdefault(k, [0, 0, 0])
            for i in range(3):
                e[i] += int(n[i])
    with (Path(work) / f"{region}-{group}.uni.tsv").open("w", encoding="utf-8") as f:
        for k in sorted(uni):
            f.write(k + "\t" + "\t".join(map(str, uni[k])) + "\n")
    files = [p.open(encoding="utf-8") for p in sorted(runs.glob(f"{group}-*-{region}-[0-9]*.tsv"))]
    with (Path(work) / f"{region}-{group}.bi.tsv").open("w", encoding="utf-8") as f:
        key, total = None, 0
        for line in heapq.merge(*files, key=lambda l: l.split("\t", 1)[0]):
            k, n = line.rstrip("\n").split("\t")
            if k != key:
                if key is not None and total >= 2:
                    f.write(f"{key}\t{total}\n")
                key, total = k, 0
            total += int(n)
        if key is not None and total >= 2:
            f.write(f"{key}\t{total}\n")
    for f in files:
        f.close()


def portuguese_counts(work, workers=6):
    """Counts every pinned input once into WORK/pt (cached): REGION-GROUP.uni.tsv and .bi.tsv for
    Brazil and Portugal (br, pt) and the news and the parliaments (leipzig, parlaibero), and
    stats.json."""
    import multiprocessing
    folder = Path(work) / "pt"
    stats_path = folder / "stats.json"
    if stats_path.exists():
        return folder
    inputs = pt_inputs(work)
    for sub in ("runs", "leipzig"):
        shutil.rmtree(folder / sub, ignore_errors=True)
        (folder / sub).mkdir(parents=True)
    archives = [p for n, p in inputs.items() if n.endswith(".tar.gz")]
    stats = {"sources": {}}
    with multiprocessing.Pool(workers) as pool:
        parla = [pool.apply_async(pt_parlaibero_count, ((inputs[f"{r.upper()}_interventions.csv"], r, folder),))
                 for r in ("br", "pt")]
        extracted = pool.map(pt_leipzig_extract, [(p, folder) for p in archives], chunksize=1)
        # A site outside .br and .pt belongs to the country whose spellings it uses at least four
        # times as often as the other's, in three sentences or more; otherwise it is left out.
        votes = collections.defaultdict(lambda: [0, 0])
        for _, v, _ in extracted:
            for host, (b, p) in v.items():
                votes[host][0] += b
                votes[host][1] += p
        sites = {h: "br" if b >= 3 and b >= 4 * p else "pt" if p >= 3 and p >= 4 * b else None
                 for h, (b, p) in votes.items()}
        # The same sentence in two archives (the 2011 newscrawls overlap) counts once, in the
        # first archive in pt-inputs.tsv's order.
        seen, jobs = set(), []
        for name, _, hashes in extracted:
            repeats = set()
            for i, h in enumerate(hashes):
                if h in seen:
                    repeats.add(i)
                seen.add(h)
            stats["sources"][name[:-len(".tar.gz")]] = {"sentences": len(hashes), "repeats": len(repeats)}
            jobs.append((name, folder, sites, repeats))
        del seen, extracted
        counted = pool.map(pt_leipzig_count, jobs, chunksize=1) + [r.get() for r in parla]
    for name, tokens in counted:
        stats["sources"].setdefault(name[len("leipzig-"):] if name.startswith("leipzig-") else name, {})["words"] = tokens
    stats["sites"] = {r: sum(1 for h, v in sites.items() if v == r and not h.endswith((".br", ".pt")))
                      for r in ("br", "pt")}
    for region in ("br", "pt"):
        for group in PT_WEIGHT:
            pt_merge(folder, region, group)
    stats_path.write_text(json.dumps(stats, indent=1, sort_keys=True) + "\n", encoding="utf-8")
    shutil.rmtree(folder / "runs")
    shutil.rmtree(folder / "leipzig")
    return folder


def pt_fold(word):
    return "".join(c for c in unicodedata.normalize("NFD", word) if not unicodedata.combining(c))


def pt_marks(word):
    return sum(a != b for a, b in zip(word, pt_fold(word)))


def pt_uni(folder, region):
    """A country's words: all uses, lowercase uses and capitalised uses, PT_WEIGHT's sum."""
    uni = {}
    for group, weight in PT_WEIGHT.items():
        for line in open(Path(folder) / f"{region}-{group}.uni.tsv", encoding="utf-8"):
            k, *n = line.rstrip("\n").split("\t")
            e = uni.setdefault(k, [0, 0, 0])
            for i in range(3):
                e[i] += weight * int(n[i])
    return uni


def pt_pairs(folder, region, vocab):
    """A country's news pairs of two words in vocab seen PT_MIN_PAIR times or more, each scaled so
    that its count over vocab's count of its first word is the news's P(second | first)."""
    news = {}
    for line in open(Path(folder) / f"{region}-leipzig.uni.tsv", encoding="utf-8"):
        k, n, _, _ = line.rstrip("\n").split("\t")
        if k in vocab:
            news[k] = int(n)
    pairs = {}
    for line in open(Path(folder) / f"{region}-leipzig.bi.tsv", encoding="utf-8"):
        k, c = line.rstrip("\n").split("\t")
        a, b = k.split(" ")
        if int(c) >= PT_MIN_PAIR and a in news and b in vocab:
            pairs[k.encode("utf-8")] = int(c) * vocab[a] / news[a]
    return pairs


def pt_words(folder, region):
    """A country's words with their counts, less the spellings Brazil's spelling retired and plain
    spellings at least 20 times rarer than a marked twin ("nao" beside "não", "voce" beside "você"),
    the rule Polish's model keeps (ios/scripts/build-typing-model.py, TWIN_RATIO), except
    PT_CONTEXT_WORDS; and the corpus's size."""
    counts, total = {}, 0
    for k, (n, _, _) in pt_uni(folder, region).items():
        total += n
        if n and len(k) <= 32 and (len(k) > 1 or k in PT_ONE_LETTER):
            counts[k] = n
    best = {}
    for w, c in counts.items():
        f = pt_fold(w)
        if c > best.get(f, (0, ""))[0]:
            best[f] = (c, w)
    retired = {w for w in counts if PT_RETIRED.fullmatch(w) and any(
        o in counts and o != w and pt_fold(o) == pt_fold(w) and pt_marks(o) < pt_marks(w)
        for o in (best[pt_fold(w)][1], pt_fold(w)))}
    twins = {w for w, c in counts.items() if w not in PT_CONTEXT_WORDS
             and best[pt_fold(w)][0] >= 20 * c and pt_marks(w) < pt_marks(best[pt_fold(w)][1])}
    return {w: c for w, c in counts.items() if w not in retired and w not in twins}, total, retired, twins


def portuguese(work, out):
    folder = portuguese_counts(work)
    words, total, retired, twins = pt_words(folder, "br")
    kept = sorted(words, key=lambda w: (-words[w], w))[: builder.MAX_WORDS["pt"]]
    vocab = {w: words[w] for w in kept}
    for w, c in builder.extra_words("pt", total).items():
        vocab[w] = max(vocab.get(w, 0), words.get(w, 0), c)
    pairs = pt_pairs(folder, "br", vocab)
    print(f"Portuguese (Brazil): {total:,} weighted words counted, {len(words):,} spellings kept ({len(retired):,} "
          f"retired in 1990 and {len(twins):,} plain twins left out), {len(vocab):,} in the model, "
          f"{len(pairs):,} pairs", flush=True)
    builder.MIN_PAIR = 0  # pt_pairs has applied PT_MIN_PAIR to the news's own counts
    builder.write_model("pt", vocab, total, pairs, str(out))


if __name__ == "__main__":
    if len(sys.argv) != 4 or sys.argv[1] not in ("de", "fr", "it", "nb", "pt"):
        sys.exit(__doc__)
    lang, work, out = sys.argv[1:]
    if lang in ("de", "fr", "it"):
        builder.main(lang, work, out)
        if lang in ELISIONS:
            refine(lang, work, out)
    elif lang == "pt":
        portuguese(work, out)
    else:
        norwegian(Path(work), Path(out))
