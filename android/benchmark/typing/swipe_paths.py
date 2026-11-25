"""Synthetic swipe paths for the held-out sentences, shared by the Android and iPhone replays so
both keyboards are scored on the very same paths. A faithful port of the iPhone swipe benchmark's
generator (2026-11-20-hex-swipe-typing/bench/main.swift: SplitMix64, Noise, swipeTimed,
keySequence and its sentence loop); with the same seed a word's path matches the Swift one to
floating-point rounding, up to a translation (see the geometry below).

Geometry: key pitches on the canonical grid both Hex keyboards share. Row r's centres sit at
y = r + 0.5; rows 0 and 1 are centred in columns = the wider of them, row 2 (between shift and
delete) likewise, so q (a on AZERTY) is at (0.5, 0.5). A replay maps a point onto its own
layout as q's centre + (point - 0.5) * pitch.

Every swipable word is written, in the model or not (the replays skip and count the words their
model lacks). The Swift bench skips those before drawing a path, so after a sentence set's first
such word its random numbers, and paths, differ from the bench's.

Writes gzip TSV: "#" header lines (lang, noise, rate, seed, rows), then one line per swiped word:
sentence, position (0-based), word, previous (the true word before; empty at a sentence's start)
and the points as space-separated x,y,t (pitches, seconds). With --words, sentence is the word's
index in the list and position the repeat.
Usage: python3 swipe_paths.py LANG OUT.tsv.gz [--noise typical|neat|sloppy|none] [--rate 120]
       [--flow F] [--hesitate H] [--limit N] [--seed S] [--words a,b,c --repeats N]"""
import argparse, gzip, math, os, sys, unicodedata

HERE = os.path.dirname(os.path.abspath(__file__))
MASK = (1 << 64) - 1
GOLDEN = 0x9E3779B97F4A7C15

# Letter rows, as android/app/src/main/java/ly/anoma/hex/keyboard/KeyboardLetters.kt draws them.
# French's apostrophe key ends row 2: a key, but not a letter.
ROWS = {"en": ["qwertyuiop", "asdfghjkl", "zxcvbnm"], "it": ["qwertyuiop", "asdfghjkl", "zxcvbnm"],
        "pt": ["qwertyuiop", "asdfghjkl", "zxcvbnm"],
        "es": ["qwertyuiop", "asdfghjklñ", "zxcvbnm"], "nb": ["qwertyuiopå", "asdfghjkløæ", "zxcvbnm"],
        "de": ["qwertzuiopü", "asdfghjklöä", "yxcvbnm"], "fr": ["azertyuiop", "qsdfghjklm", "wxcvbn'"]}
# Added to seed * 7919 for the sentence loop (en and es as the Swift bench has them).
SEED_OFFSET = {"en": 0, "es": 17, "de": 29, "nb": 31, "fr": 37, "it": 41, "pt": 43}
# Letters with neither a key nor a base letter's key, swiped as these keys.
SPELLED = {"ß": "s", "œ": "oe", "æ": "ae"}


class SplitMix64:
    """Swift's SplitMix64, with its 64-bit wrapping arithmetic."""

    def __init__(self, seed):
        self.state = (seed + GOLDEN) & MASK

    def next(self):
        self.state = (self.state + GOLDEN) & MASK
        z = self.state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return z ^ (z >> 31)

    def unit(self):
        return (self.next() >> 11) / float(1 << 53)

    def uniform(self, a, b):
        return a + (b - a) * self.unit()

    def gaussian(self):
        u1 = max(self.unit(), 1e-12)
        u2 = self.unit()
        return math.sqrt(-2 * math.log(u1)) * math.cos(2 * math.pi * u2)


# Noise presets in key pitches, as Swift's Noise (see its comments for each field).
TYPICAL = dict(sloppiness=0.25, offset=0.08, via=0.22, start=0.18, end=0.22, cut=0.5, overshoot=(-0.1, 0.35),
               tremor=0.025, rate=120.0, speed=(0.6, 1.5), flow=0.0, hesitate=0.0)
NOISE = {
    "typical": TYPICAL,
    "sloppy": {**TYPICAL, "offset": 0.15, "via": 0.36, "start": 0.3, "end": 0.42, "cut": 0.8,
               "overshoot": (-0.25, 0.7), "tremor": 0.055, "speed": (0.45, 1.2)},
    "none": {**TYPICAL, "sloppiness": 0.0, "offset": 0.0, "via": 0.0, "start": 0.0, "end": 0.0, "cut": 0.0,
             "overshoot": (0.0, 0.0), "tremor": 0.0, "speed": (1.0, 1.0)},
    "neat": {**TYPICAL, "offset": 0.04, "via": 0.12, "start": 0.1, "end": 0.12, "cut": 0.3,
             "overshoot": (-0.05, 0.15), "tremor": 0.01},
}


class Layout:
    """A layout's letter centres and its "starts on a letter" test, in pitches."""

    def __init__(self, lang):
        self.rows = ROWS[lang]
        self.columns = max(len(self.rows[0]), len(self.rows[1]))
        self.side = (self.columns - len(self.rows[2])) / 2
        self.centres = {}
        for r, row in enumerate(self.rows):
            left = (self.columns - len(row)) / 2
            for i, c in enumerate(row):
                if c != "'":
                    self.centres[c] = (left + i + 0.5, r + 0.5)

    def on_letter(self, x, y):
        # Rows 0 and 1 are letters edge to edge (edge keys reach the row ends); row 2 between
        # shift and delete, less the apostrophe key's cell.
        if not 0 <= y < 3:
            return False
        if y < 2:
            return 0 <= x < self.columns
        if not self.side <= x < self.columns - self.side:
            return False
        return self.rows[2][min(len(self.rows[2]) - 1, int(x - self.side))] != "'"

    def key_sequence(self, word):
        """The keys a word's swipe goes through (an accented letter on its base letter's key
        unless it has its own), repeats run together; None for a letter with no key."""
        out = []
        for ch in word:
            if ch == "'" or unicodedata.combining(ch):
                continue
            base = unicodedata.normalize("NFD", ch)[0]
            keys = ch if ch in self.centres else base if base in self.centres else SPELLED.get(ch)
            if keys is None or any(k not in self.centres for k in keys):
                return None
            for k in keys:
                if not out or out[-1] != k:
                    out.append(k)
        return out


def length(x, y):
    return math.sqrt(x * x + y * y)  # not hypot: Swift's (x*x + y*y).squareRoot(), bit for bit


def swipe_timed(centres, noise, s, rng, on_letter):
    """Swift's swipeTimed: via points around each key, corners rounded with quadratic Béziers,
    an overshoot or a trim at the end, a slow wobble, then sampled in time on a quintic
    (minimum-jerk) profile between keys, with an optional hesitation. Returns ([(x, y)], [t]).
    Random numbers are drawn in Swift's order, and float operations kept in its order."""
    m = len(centres)
    ox = rng.gaussian() * noise["offset"] * s
    oy = rng.gaussian() * noise["offset"] * s
    via = []
    for j, (cx, cy) in enumerate(centres):
        sd = (noise["start"] if j == 0 else noise["end"] if j == m - 1 else noise["via"]) * s
        gx = rng.gaussian() * sd
        vx, vy = (cx + ox) + gx, (cy + oy) + rng.gaussian() * sd
        # A swipe starts on a letter key.
        tries = 0
        while j == 0 and not on_letter(vx, vy) and tries < 50:
            gx = rng.gaussian() * sd
            vx, vy = (cx + ox * 0.5) + gx, (cy + oy * 0.5) + rng.gaussian() * sd
            tries += 1
        if j == 0 and not on_letter(vx, vy):
            vx, vy = cx, cy
        via.append((vx, vy))

    # Dense path with rounded corners; anchors are the path indices of each key's moment.
    path, anchors = [via[0]], [0]

    def line_to(p):
        ax, ay = path[-1]
        steps = max(1, int(length(p[0] - ax, p[1] - ay) / 0.05))
        for k in range(1, steps + 1):
            path.append((ax + (p[0] - ax) * (k / steps), ay + (p[1] - ay) * (k / steps)))

    for j in range(1, m - 1):
        (ax, ay), (bx, by), (cx, cy) = via[j - 1], via[j], via[j + 1]
        lin, lout = length(bx - ax, by - ay), length(cx - bx, cy - by)
        if not (lin > 1e-6 and lout > 1e-6):
            line_to((bx, by))
            anchors.append(len(path) - 1)
            continue
        # Fingers round a corner by a few tenths of a key, however long the strokes around it.
        d = min(0.45 * min(lin, lout), noise["cut"] * min(s, 1.8) * rng.uniform(0.4, 1.4) * 0.6)
        pin = (bx - (bx - ax) * (d / lin), by - (by - ay) * (d / lin))
        pout = (bx + (cx - bx) * (d / lout), by + (cy - by) * (d / lout))
        line_to(pin)
        for k in range(1, 13):
            t = k / 12
            a, b, c = (1 - t) * (1 - t), 2 * (1 - t) * t, t * t
            path.append((pin[0] * a + bx * b + pout[0] * c, pin[1] * a + by * b + pout[1] * c))
            if k == 6:
                anchors.append(len(path) - 1)
    line_to(via[m - 1])

    # Overshoot along the last direction, or stop short (keeping half the last stretch).
    e = rng.uniform(*noise["overshoot"]) * s
    if len(path) >= 2:
        tx, ty = path[max(0, len(path) - 6)]
        dx, dy = path[-1][0] - tx, path[-1][1] - ty
        dl = length(dx, dy)
        if dl > 1e-6:
            if e > 0:
                line_to((path[-1][0] + dx * (e / dl), path[-1][1] + dy * (e / dl)))
            else:
                trimmed, last = 0.0, anchors[-1]
                keep = last + (len(path) - 1 - last) // 2
                while len(path) - 1 > keep and trimmed < -e:
                    trimmed += length(path[-1][0] - path[-2][0], path[-1][1] - path[-2][1])
                    path.pop()
    anchors.append(len(path) - 1)

    # Arc lengths, and a slow wobble across the path fading in and out at its ends.
    arc = [0.0]
    for i in range(1, len(path)):
        arc.append(arc[i - 1] + length(path[i][0] - path[i - 1][0], path[i][1] - path[i - 1][1]))
    total = arc[-1]
    if noise["tremor"] > 0 and total > 0:
        amp = noise["tremor"] * s
        l1 = rng.uniform(1.2, 3.0)
        l2 = rng.uniform(0.5, 1.2)
        p1 = rng.uniform(0, 2 * math.pi)
        p2 = rng.uniform(0, 2 * math.pi)
        wobbled = list(path)
        for i in range(1, len(path) - 1):
            dx, dy = path[i + 1][0] - path[i - 1][0], path[i + 1][1] - path[i - 1][1]
            dl = length(dx, dy)
            if not dl > 1e-9:
                continue
            fade = min(1, arc[i] / 0.3, (total - arc[i]) / 0.3)
            w = amp * fade * (math.sin(2 * math.pi * arc[i] / l1 + p1) + 0.5 * math.sin(2 * math.pi * arc[i] / l2 + p2))
            wobbled[i] = (path[i][0] + (-dy / dl) * w, path[i][1] + (dx / dl) * w)
        path = wobbled

    # Time: each stretch between key moments takes (0.08 + 0.05 * its length) * speed.
    speed = rng.uniform(*noise["speed"])
    times = [0.0]
    for k in range(1, len(anchors)):
        times.append(times[k - 1] + (0.08 + 0.05 * (arc[anchors[k]] - arc[anchors[k - 1]])) * speed)

    def position(u):
        lo, hi = 0, len(arc) - 1
        while lo < hi - 1:
            mid = (lo + hi) // 2
            if arc[mid] <= u:
                lo = mid
            else:
                hi = mid
        span = arc[hi] - arc[lo]
        t = min(1, max(0, (u - arc[lo]) / span)) if span > 0 else 0
        return (path[lo][0] + (path[hi][0] - path[lo][0]) * t, path[lo][1] + (path[hi][1] - path[lo][1]) * t)

    # Speed kept through each key: flow of the slower neighbouring stretch's peak, by cos²(turn/2).
    through = [0.0] * len(anchors)
    if noise["flow"] > 0 and len(anchors) == m:
        for j in range(1, m - 1):
            ax, ay = via[j][0] - via[j - 1][0], via[j][1] - via[j - 1][1]
            bx, by = via[j + 1][0] - via[j][0], via[j + 1][1] - via[j][1]
            la, lb = length(ax, ay), length(bx, by)
            if not (la > 1e-6 and lb > 1e-6):
                continue
            cos_turn = max(-1, min(1, (ax * bx + ay * by) / (la * lb)))
            keep = noise["flow"] * (1 + cos_turn) / 2
            before = (arc[anchors[j]] - arc[anchors[j - 1]]) / max(1e-9, times[j] - times[j - 1])
            after = (arc[anchors[j + 1]] - arc[anchors[j]]) / max(1e-9, times[j + 1] - times[j])
            through[j] = keep * 1.5 * min(before, after)

    # A sample every 1/rate s, each stretch walked on a quintic Hermite (minimum jerk from rest).
    out, stamps = [], []
    end, t, k = times[-1], 0.0, 1
    while t < end:
        while k < len(times) - 1 and t > times[k]:
            k += 1
        span = max(1e-9, times[k] - times[k - 1])
        tau = (t - times[k - 1]) / span
        stretch = arc[anchors[k]] - arc[anchors[k - 1]]
        t2 = tau * tau
        t3 = t2 * tau
        t4 = t3 * tau
        t5 = t4 * tau
        h5 = 10 * t3 - 15 * t4 + 6 * t5
        h1 = tau - 6 * t3 + 8 * t4 - 3 * t5
        h4 = -4 * t3 + 7 * t4 - 3 * t5
        walked = stretch * h5 + through[k - 1] * span * h1 + through[k] * span * h4
        out.append(position(arc[anchors[k - 1]] + max(0, min(stretch, walked))))
        stamps.append(t)
        t += 1 / noise["rate"]
    out.append(path[-1])
    stamps.append(end)
    # Sometimes the finger stops once along the way, for 0.08 to 0.25 s.
    if noise["hesitate"] > 0 and rng.unit() < noise["hesitate"] and len(out) > 8:
        at = int(len(out) * rng.uniform(0.15, 0.85))
        wait = rng.uniform(0.08, 0.25)
        for i in range(at, len(stamps)):
            stamps[i] += wait
    return out, stamps


def points_text(layout, points, stamps):
    # x is clamped to the layout's width, as the Swift bench clamps to its layout's.
    return " ".join(f"{min(max(x, 0), layout.columns):.5f},{y:.5f},{t:.5f}" for (x, y), t in zip(points, stamps))


def main():
    ap = argparse.ArgumentParser(description="Synthetic swipe paths for the held-out sentences.")
    ap.add_argument("lang", choices=sorted(ROWS))
    ap.add_argument("out")
    ap.add_argument("--noise", choices=sorted(NOISE), default="typical")
    ap.add_argument("--rate", type=float, default=120.0)
    ap.add_argument("--flow", type=float, default=0.0)
    ap.add_argument("--hesitate", type=float, default=0.0)
    ap.add_argument("--limit", type=int)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--words", help="comma-separated words, each swiped --repeats times (for checks)")
    ap.add_argument("--repeats", type=int, default=8)
    a = ap.parse_args()
    noise = {**NOISE[a.noise], "rate": a.rate, "flow": a.flow, "hesitate": a.hesitate}
    layout = Layout(a.lang)
    counts = {"words": 0, "swiped": 0, "one-key": 0, "no key": 0}

    def swipe(seq, s, rng):
        return swipe_timed([layout.centres[c] for c in seq], noise, s, rng, layout.on_letter)

    with gzip.open(a.out, "wt", encoding="utf-8", newline="\n") as f:
        f.write(f"# lang {a.lang}\n# noise {a.noise} "
                + " ".join(f"{k}={v}" for k, v in noise.items() if k != "rate") + "\n")
        f.write(f"# rate {a.rate:g}\n# seed {a.seed}\n# rows {'/'.join(layout.rows)}\n")
        f.write("# sentence\tposition\tword\tprevious\tpoints (x,y,t: key pitches from the layout's top left, seconds)\n")
        if a.words:
            # As Swift's --words: one generator from the seed, a sloppiness drawn for every swipe.
            rng = SplitMix64(a.seed & MASK)
            for i, w in enumerate(a.words.split(",")):
                seq = layout.key_sequence(w)
                if seq is None or len(seq) < 2:
                    print(w, "one key", file=sys.stderr)
                    continue
                for r in range(a.repeats):
                    pts, ts = swipe(seq, math.exp(rng.gaussian() * noise["sloppiness"]), rng)
                    f.write(f"{i}\t{r}\t{w}\t\t{points_text(layout, pts, ts)}\n")
            return
        with gzip.open(os.path.join(HERE, "held-out", f"{a.lang}.tsv.gz"), "rt", encoding="utf-8") as src:
            sentences = [line.rstrip("\n").split("\t")[2].lower() for line in src]
        rng = SplitMix64((a.seed * 7919 + SEED_OFFSET[a.lang]) & MASK)
        for n, sentence in enumerate(sentences[:a.limit]):
            words = [w for w in sentence.split(" ") if w]  # as Swift's split: no empty words
            sloppiness = math.exp(rng.gaussian() * noise["sloppiness"])
            previous = ""
            for position, w in enumerate(words):
                counts["words"] += 1
                seq = layout.key_sequence(w)
                if seq is None or len(seq) < 2:
                    counts["no key" if seq is None else "one-key"] += 1
                else:
                    counts["swiped"] += 1
                    pts, ts = swipe(seq, sloppiness, rng)
                    f.write(f"{n}\t{position}\t{w}\t{previous}\t{points_text(layout, pts, ts)}\n")
                previous = w
    print(" ".join(f"{k} {v}" for k, v in counts.items()), file=sys.stderr)


if __name__ == "__main__":
    main()
