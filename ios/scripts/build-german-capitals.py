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
the capitalized share rounds to at least SOMETIMES (50%) and the two counts together reach
MIN_TOTAL (40 occurrences across 40 years of books, below which the share is mostly noise),
except pronouns and possessives that are capitalized for other reasons (FUNCTION_WORDS).

The counts decide every percentage; the caps below are applied after them, and the merged
counts are kept, so a rule can change without streaming the shards again. Three kinds of
word are capped at CAP (94%) so the keyboard only capitalizes them after an article or
determiner, because they are also common lowercase words:

- a verb form: a word ending in "e" whose word+"n" is its infinitive, where both readings
  are used enough to type. The infinitive's own capitalised share can't tell them apart
  ("Fragen" the plural outnumbers "fragen" the verb, 74%), so the rule reads the counts
  instead: the infinitive needs VERB_MIN_LOW lowercase uses, the -e word at least
  VERB_MIN_SHARE of its own uses lowercase (frage 5.4%, reise 2.7%), and the infinitive
  between VERB_MIN_RATIO and VERB_MAX_RATIO times the lowercase -e word, which is the band
  a conjugation fills (frage 2.8x, drucke 23x). A noun without a verb of its own is outside
  it: "hause" is 0.6% lowercase, and "wegen" and "lagen" are 250x and 43x the lowercase
  "wege" and "lage", so "haus", "hause", "wege" and "lage" keep their capitals.
- a stem: the same test for a consonant stem and its word+"en" ("kauf"/"kaufen", "schlaf"/
  "schlafen", "sitz"/"sitzen"), the imperative and casual first person people type. Most
  nouns are built from a verb stem too, so its band is wider (STEM_*): "kaufen" may be 38x
  the lowercase "kauf". A noun the books nearly always capitalize stays: "bild" is 0.6%
  lowercase, "bericht" 1.1%, "tisch" has "tischen" at only 0.9x it.
- ALSO_LOWERCASE: words whose lowercase use is common but no rule above sees it, and every
  two-letter word, which is a chat abbreviation ("gl", "rn"), never a noun.

Each line is `word<TAB>percent`, sorted by word, with a trailing newline.

The eight shards are streamed through curl and gzip -dc, four at a time, and never stored.
Each shard's counts are cached under .build-german-capitals/ beside this script, so an
interrupted run resumes where it stopped. Their merged per-token counts stay there as
counts.tsv after the run, so a later rule change renders in seconds.
"""
import hashlib, multiprocessing, os, struct, subprocess, sys

BASE = "https://storage.googleapis.com/books/ngrams/books/20200217"
FILES = ("ger", 8)          # ger/1-0000N-of-00008.gz, the German unigram shards
FIRST_YEAR, LAST_YEAR = 1980, 2019
MIN_TOTAL = 40              # occurrences (lowercase + capitalized) a word needs
SOMETIMES = 50              # the share from which a word is written to the list
CAP = 94                    # a capped word is below the keyboard's 95% "always" share
VERB_MIN_LOW = 200_000      # lowercase uses of the infinitive over 40 years, 5,000 a year
VERB_MIN_SHARE = 14         # in tenths of a percent: 1.4% of the word's uses lowercase
VERB_MIN_RATIO = 2          # the infinitive at least twice the lowercase -e form
VERB_MAX_RATIO = 24         # and at most 24 times it
STEM_MIN_LOW = 140_000      # a consonant infinitive's lowercase uses, 3,500 a year
STEM_MIN_SHARE = 13         # in tenths of a percent: 1.3% of the stem's uses lowercase
STEM_MIN_RATIO = 2          # the infinitive at least twice the lowercase stem
STEM_MAX_RATIO = 60         # and at most 60 times it: the imperative is rarer than "ich kaufe"
SHARDS = 4                  # shards streamed at once
WORK = os.path.join(os.path.dirname(os.path.abspath(__file__)), ".build-german-capitals")
FUNCTION_WORDS = frozenset("""
    sie ihr ihre ihren ihrem ihrer ihres ihnen du dich dir dein deine deinen deinem deiner
    deines euch euer eure euren eurem eurer eures ich mich mir wir uns er es ihn ihm man
    sich mein meine meinen meinem meiner meines
""".split())
# Words the books capitalize nearly always whose lowercase use is still common, and that
# the two rules above cannot see:
# - "teil" and "preis" stand alone as the separable prefixes of "teilnehmen" and
#   "preisgeben" ("ich nehme teil", "er gibt es preis");
# - "klasse" is the adjective and adverb before "Klasse" ("das ist klasse");
# - filme, rolle, presse, pumpe, campe, pflanze, beichte, fische and taufe are first person and
#   imperative too ("ich filme", "roll!"), but their infinitives are too rare or their -e
#   word is too rarely lowercase for the verb rule. "erbe" stays out: people type "das Erbe"
#   and "der Erbe".
ALSO_LOWERCASE = frozenset("""
    beichte  campe  filme  fische  klasse  pflanze  presse  preis  pumpe  rolle  taufe  teil
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


def read_counts(path):
    counts = {}
    for line in open(path, "rb"):
        token, count = line.rstrip(b"\n").split(b"\t")
        counts[token] = int(count)
    return counts


def model_counts(model_path):
    """The merged corpus counts of every token the rules can ask for, from the cache if kept.

    Every model word and its capitalized spelling is wanted: a rule reads a word's own
    share, and the word plus a verb ending ("fragen", "teilen"). The cache is keyed by that
    token set, so an interrupted run for another model never leaves counts this one trusts.
    """
    words = model_words(model_path)
    tokens = set()
    for w in words:
        tokens.add(w.encode("utf-8"))
        cap = capitalized(w)
        if cap:
            tokens.add(cap.encode("utf-8"))
    tokens = frozenset(tokens)
    work = os.path.join(WORK, hashlib.sha256(b"\n".join(sorted(tokens))).hexdigest()[:16])
    merged = os.path.join(work, "counts.tsv")
    if os.path.exists(merged):
        counts = read_counts(merged)
        print(f"{merged}: {len(counts):,} tokens counted earlier", flush=True)
        return words, counts
    os.makedirs(work, exist_ok=True)
    code, count = FILES
    urls = [f"{BASE}/{code}/1-{i:05d}-of-{count:05d}.gz" for i in range(count)]
    with multiprocessing.Pool(SHARDS, initializer=load_wanted, initargs=(tokens,)) as pool:
        parts = pool.map(count_shard, [(i, url, work) for i, url in enumerate(urls)])
    counts = merge_counts(parts)
    with open(merged + ".part", "wb") as f:
        for token, count in counts.items():
            f.write(token + b"\t" + str(count).encode() + b"\n")
    os.replace(merged + ".part", merged)
    for part in parts:
        os.remove(part)
    return words, counts


def share_of(word, low, cap):
    """The share of the word's corpus occurrences written with a capital, 0-100."""
    total = low[word] + cap[word]
    return percent(cap[word], total) if total else 0


def conjugation(word, low, cap, infinitive, min_low, min_share, min_ratio, max_ratio):
    """Whether the lowercase `word` and `infinitive` look like one verb's conjugation.

    The infinitive has to be common in the books, the word itself has to be typed lowercase
    often enough to be a verb of its own, and it has to be between `min_ratio` and
    `max_ratio` times the lowercase word, the band a real conjugation fills.
    """
    if infinitive not in low or low[infinitive] < min_low:
        return False
    total = low[word] + cap[word]
    if low[word] * 1000 < total * min_share:
        return False
    return min_ratio * low[word] <= low[infinitive] <= max_ratio * low[word]


def verb_form(word, low, cap):
    """Whether `word` is also the common lowercase first person and imperative of a verb.

    "frage" is "fragen" (ich frage, frage!), "reise" is "reisen", "drucke" is "drucken".
    A plural noun takes the same ending, so the infinitive's capitalised share says nothing
    ("Fragen" outnumbers "fragen"), and a noun can sit next to an unrelated verb form
    ("gebete" next to "gebeten"). The counts tell a conjugation from those. "hause" (0.6%
    lowercase) and "hausen" fall outside it, so "haus" and "hause" keep their capitals; so do
    "wege" and "lage", whose -n forms are the preposition "wegen" and the past "lagen".
    """
    if not word.endswith("e"):
        return False
    return conjugation(word, low, cap, word + "n",
                       VERB_MIN_LOW, VERB_MIN_SHARE, VERB_MIN_RATIO, VERB_MAX_RATIO)


def stem_form(word, low, cap):
    """Whether `word` is a consonant stem people type alone as a verb.

    "kauf" is "kaufen" ("ich kauf noch brot"), "schlaf" is "schlafen" ("schlaf gut"), "sitz"
    is "sitzen", "druck" is "drucken". Most German nouns are built from a verb stem too
    ("bild"/"bilden", "erfolg"/"erfolgen"), so the band is wider and the infinitive may be
    much more common than the stem (kaufen 38x "kauf", spielen 47x "spiel"), but a noun the
    books capitalize nearly always keeps its capital: "bild" is 0.6% lowercase, "bericht"
    1.1%, "tisch" 1.8% with "tischen" only 0.9x it, and "haus", "zeit" and "freund" fall out
    by their own or their -en form's counts. The words the counts still miss are in
    ALSO_LOWERCASE.
    """
    if word.endswith("e"):
        return False
    return conjugation(word, low, cap, word + "en",
                       STEM_MIN_LOW, STEM_MIN_SHARE, STEM_MIN_RATIO, STEM_MAX_RATIO)


def build(model_path, out_path):
    words, counts = model_counts(model_path)
    low = {w: counts.get(w.encode("utf-8"), 0) for w in words}
    cap = {}
    for w in words:
        spelling = capitalized(w)
        cap[w] = counts.get(spelling.encode("utf-8"), 0) if spelling else 0
    rows = []
    capped = []
    for w in words:
        if w in FUNCTION_WORDS:
            continue
        total = low[w] + cap[w]
        if total < MIN_TOTAL:
            continue
        p = share_of(w, low, cap)
        if p < SOMETIMES:
            continue
        if p > CAP:
            # A two-letter "word" is an abbreviation ("gl", "rn"), never a noun.
            if len(w) == 2:
                reason = "short"
            elif w in ALSO_LOWERCASE:
                reason = "list"
            elif verb_form(w, low, cap):
                reason = "verb"
            elif stem_form(w, low, cap):
                reason = "stem"
            else:
                reason = None
            if reason:
                p = CAP
                capped.append((w, total, reason))
        rows.append((w, p))
    rows.sort()
    with open(out_path + ".part", "w", encoding="utf-8", newline="\n") as f:
        for w, p in rows:
            f.write(f"{w}\t{p}\n")
    os.replace(out_path + ".part", out_path)
    high = sum(1 for _, p in rows if p >= CAP + 1)
    capped.sort(key=lambda row: -row[1])
    counts_by_reason = {
        reason: sum(1 for _, _, r in capped if r == reason)
        for reason in ("verb", "stem", "list", "short")
    }
    print(f"{out_path}: {len(rows):,} of {len(words):,} words, {high:,} at 95% or more, "
          f"{len(rows) - high:,} at 50-94%, {len(counts):,} tokens summed")
    print(f"{len(capped):,} words moved from 95% or more to {CAP}% "
          f"({counts_by_reason['verb']} verb forms, {counts_by_reason['stem']} stems, "
          f"{counts_by_reason['list']} listed, {counts_by_reason['short']} short):")
    for w, total, reason in capped[:250]:
        print(f"    {w} ({total:,}, {reason})")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    try:
        build(*sys.argv[1:])
    except BaseException:
        print(f"leaving {WORK} for a resume", file=sys.stderr)
        raise
