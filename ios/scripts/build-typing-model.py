#!/usr/bin/env python3
"""Builds a spatial decoder model (Experiments › Full keyboard › Spatial autocorrect) from a
word-count list and a bigram-count list.

    scripts/build-typing-model.py en words.txt bigrams.txt HEXKeyboard/TypingModel/en.hextm

words.txt: "word count" per line; bigrams.txt: "word1 word2 count" per line (UTF-8, any
case; words are lowercased and kept only when made of letters and at most one apostrophe).
English uses SymSpell's frequency_dictionary_en_82_765.txt and
frequency_bigramdictionary_en_243_342.txt (MIT; from Google Books Ngram, CC BY 3.0, and SCOWL).

Format v2 (little-endian), memory-mapped by TypingDecoder.swift:
    "HXTM" u32 version=2, u32 words N, u32 bigrams B, u32 blob bytes, u32 alphabet size A,
    char[4] language (e.g. "en\0\0")
    A × (u32 Unicode scalar, u8 code, u8 base code, u16 0)   codes 1…A by scalar; base code
        is the unaccented letter's (é → e) or the letter's own
    u32[N+1] word offsets into the blob (words sorted by code bytes)
    u8[N] unigram cost, u8 padding to 8, u64[B] (w1 << 32 | w2) sorted, u8[B] bigram cost
    blob: each word as one code byte per letter
Costs are -ln(p) in eighths of a nat, clamped to 255.
"""
import math, struct, sys, unicodedata

# Bigram counts may be on a larger scale than unigram counts (about 40x in SymSpell's
# lists), so P(w2 | w1) = c(w1 w2) / max(sum over w2, SCALE * c(w1)).
SCALE = 40.0


def cost(p):
    return max(0, min(255, round(-math.log(max(p, 1e-30)) * 8)))


def is_word(w):
    return w.count("'") <= 1 and not w.startswith("'") and not w.endswith("'") and all(c.isalpha() or c == "'" for c in w)


def main(language, unigram_path, bigram_path, out_path, scale=SCALE):
    counts = {}
    for line in open(unigram_path, encoding="utf-8-sig"):
        parts = line.split()
        if len(parts) == 2:
            w = parts[0].lower()
            if is_word(w):
                counts[w] = counts.get(w, 0) + int(parts[1])
    scalars = sorted({c for w in counts for c in w})
    if len(scalars) > 250:
        sys.exit("alphabet too large")
    code = {c: i + 1 for i, c in enumerate(scalars)}
    def base(c):
        b = unicodedata.normalize("NFD", c)[0]
        return code.get(b, code[c])
    enc = {w: bytes(code[c] for c in w) for w in counts}
    words = sorted(counts, key=lambda w: enc[w])
    index = {w: i for i, w in enumerate(words)}
    total = sum(counts.values())

    pairs = {}
    for line in open(bigram_path, encoding="utf-8-sig"):
        parts = line.split()
        if len(parts) == 3:
            a, b = parts[0].lower(), parts[1].lower()
            if a in index and b in index:
                pairs[(a, b)] = pairs.get((a, b), 0) + int(parts[2])
    sums = {}
    for (a, _), c in pairs.items():
        sums[a] = sums.get(a, 0) + c
    bigrams = sorted(((index[a] << 32) | index[b], cost(c / max(sums[a], scale * counts[a]))) for (a, b), c in pairs.items())

    blob = bytearray()
    offsets = []
    for w in words:
        offsets.append(len(blob))
        blob += enc[w]
    offsets.append(len(blob))

    out = bytearray(b"HXTM")
    out += struct.pack("<IIIII", 2, len(words), len(bigrams), len(blob), len(scalars))
    out += language.encode("ascii")[:4].ljust(4, b"\0")
    for c in scalars:
        out += struct.pack("<IBBH", ord(c), code[c], base(c), 0)
    out += struct.pack(f"<{len(offsets)}I", *offsets)
    out += bytes(cost(counts[w] / total) for w in words)
    out += b"\0" * (-len(out) % 8)
    out += struct.pack(f"<{len(bigrams)}Q", *(k for k, _ in bigrams))
    out += bytes(c for _, c in bigrams)
    out += blob
    with open(out_path, "wb") as f:
        f.write(out)
    print(f"{out_path}: {language}, {len(words)} words, {len(bigrams)} bigrams, {len(scalars)} letters, {len(out) / 1e6:.2f} MB")


if __name__ == "__main__":
    if len(sys.argv) not in (5, 6):
        sys.exit(__doc__)
    main(*sys.argv[1:5], *(float(a) for a in sys.argv[5:]))
