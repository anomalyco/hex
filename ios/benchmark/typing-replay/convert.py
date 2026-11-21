#!/usr/bin/env python3
"""Convert public datasets of real typing into TypingReplayTests recordings.

The tests read one of two shapes, both in the unit grid described in
ios/HEXMobileTests/TypingReplayTests.swift. x 0..1 spans the ten columns of the
top QWERTY letter row (q's left edge to p's right edge), y 0..1 spans the three
letter rows, and rows two and three start half and one and a half columns in.

Inputs are files downloaded by hand; this script never makes a network request.
Each subcommand prints what it read, emitted and skipped to stderr.
"""
import argparse, csv, io, json, sys, zipfile
from collections import Counter, OrderedDict
from pathlib import Path

ROWS = ("qwertyuiop", "asdfghjkl", "zxcvbnm")
ROW_OFFSETS = (0.0, 0.5, 1.5)


def point(x, y, t):
    return (round(float(x), 5), round(float(y), 5), round(float(t), 3))


def gesture(points):
    points = list(points)
    return {"x": [p[0] for p in points], "y": [p[1] for p in points], "t": [p[2] for p in points]}


class Writer:
    """Writes the sentence array without holding every gesture in memory."""

    def __init__(self, path):
        self.file = open(path, "w", encoding="utf-8")
        self.file.write('{"sentences":[')
        self.count = 0

    def write(self, source, session, words, key, items):
        if self.count:
            self.file.write(",")
        self.file.write(json.dumps({"source": source, "session": session, "words": words, key: items},
                                   separators=(",", ":")))
        self.count += 1

    def close(self):
        self.file.write("]}\n")
        self.file.close()


def limited(count, limit):
    return limit is not None and count >= limit


def futo(paths, writer, limit):
    """One JSONL record per word; records for a sentence may have gaps."""
    stats, done = Counter(), False
    for path in paths:
        source = "futo-" + Path(path).stem
        groups = OrderedDict()
        with open(path, encoding="utf-8") as file:
            for line in file:
                if limited(writer.count, limit):
                    done = True
                    break
                if not line.strip():
                    continue
                record = json.loads(line)
                stats["records"] += 1
                if record.get("dual_finger"):
                    stats["dual"] += 1
                    continue
                if record.get("layout", "qwerty") != "qwerty":
                    stats["layout"] += 1
                    continue
                if record.get("language", "en") != "en":
                    stats["language"] += 1
                    continue
                if not isinstance(record.get("data"), list):
                    stats["malformed"] += 1
                    continue
                sentence = record["sentence"]
                group = groups.setdefault((record["session"], sentence), {})
                group[record["word_idx"]] = record
                words = sentence.split()
                if not all(index in group for index in range(len(words))):
                    if len(groups) > 128:
                        groups.popitem(last=False)
                        stats["stale"] += 1
                    continue
                del groups[(record["session"], sentence)]
                if any(group[index].get("word") != word for index, word in enumerate(words)):
                    stats["mismatch"] += 1
                    continue
                gestures = []
                for index in range(len(words)):
                    data = sorted(group[index]["data"], key=lambda sample: sample["t"])
                    if len(data) < 2:
                        gestures = []
                        break
                    start = data[0]["t"]
                    gestures.append(gesture(point(s["x"], s["y"], s["t"] - start) for s in data))
                if not gestures:
                    stats["malformed"] += 1
                    continue
                writer.write(source, record["session"], words, "gestures", gestures)
                stats["words"] += len(words)
        stats["incomplete"] += len(groups)
        if done:
            break
    print(
        f"futo: {writer.count} sentences, {stats['words']} words; skipped {stats['dual']} dual-finger, "
        f"{stats['layout']} non-qwerty, {stats['language']} non-English, {stats['incomplete']} incomplete, "
        f"{stats['mismatch']} word mismatches, {stats['malformed']} malformed, {stats['stale']} stale groups; "
        f"{stats['records']} records", file=sys.stderr,
    )


def howweswipe_rows(stream):
    for line in stream:
        row = line.split()
        if len(row) < 12 or row[0] == "sentence" or row[11] not in ("0", "1"):
            continue
        try:
            yield {"sentence": row[0], "time": int(row[1]), "width": float(row[2]),
                   "height": float(row[3]), "event": row[4], "x": float(row[5]),
                   "y": float(row[6]), "word": row[10]}
        except ValueError:
            continue


def howweswipe_chunks(rows):
    """Runs of consecutive touchmoves for one word, split on word change or touchend."""
    chunks, current, word = [], [], None

    def flush():
        nonlocal current
        if current:
            chunks.append(current)
        current = []

    for row in rows:
        if row["event"] == "touchend":
            flush()
            word = row["word"]
        elif row["event"] == "touchmove":
            if word is not None and row["word"] != word:
                flush()
            word = row["word"]
            current.append(row)
    flush()
    return chunks


def howweswipe_log(stream, session, writer, limit, stats):
    sentences = OrderedDict()
    for row in howweswipe_rows(stream):
        stats["rows"] += 1
        sentences.setdefault(row["sentence"], []).append(row)
    for sentence, rows in sentences.items():
        if limited(writer.count, limit):
            return
        words = sentence.split("_")
        chunks = howweswipe_chunks(rows)
        stats["chunks"] += len(chunks)
        chosen, cursor = [], 0
        for word in words:
            while cursor < len(chunks) and chunks[cursor][0]["word"] != word:
                cursor += 1
            last = None
            while cursor < len(chunks) and chunks[cursor][0]["word"] == word:
                last = chunks[cursor]
                cursor += 1
            if last is None:
                chosen = []
                break
            chosen.append(last)
        if not chosen:
            stats["incomplete"] += 1
            continue
        gestures = []
        for rows in chosen:
            rows = sorted(rows, key=lambda row: row["time"])
            if len(rows) < 2:
                gestures = []
                break
            start = rows[0]["time"]
            # The study's web keyboard: ten columns across the canvas, three letter
            # rows of (height - 4) / 3.7 starting one pixel down.
            pitch = (rows[0]["height"] - 4) / 3.7
            gestures.append(gesture(
                point(row["x"] / rows[0]["width"], (row["y"] - 1) / (3 * pitch), row["time"] - start)
                for row in rows
            ))
        if not gestures:
            stats["short"] += 1
            continue
        writer.write("howweswipe", session, words, "gestures", gestures)
        stats["words"] += len(words)


def howweswipe_streams(paths):
    for path in map(Path, paths):
        if path.is_dir():
            for member in sorted(path.rglob("*.log")):
                with open(member, encoding="utf-8") as stream:
                    yield member.stem, stream
        elif path.suffix == ".zip":
            with zipfile.ZipFile(path) as archive:
                for info in sorted(archive.infolist(), key=lambda item: item.filename):
                    if info.filename.endswith(".log"):
                        with archive.open(info) as raw:
                            yield Path(info.filename).stem, io.TextIOWrapper(raw, encoding="utf-8")
        else:
            with open(path, encoding="utf-8") as stream:
                yield path.stem, stream


def howweswipe(paths, writer, limit):
    stats = Counter()
    for session, stream in howweswipe_streams(paths):
        if limited(writer.count, limit):
            break
        howweswipe_log(stream, session, writer, limit, stats)
        stats["logs"] += 1
    print(
        f"howweswipe: {writer.count} sentences, {stats['words']} words, {stats['logs']} logs, "
        f"{stats['rows']} rows, {stats['chunks']} swipes; skipped {stats['incomplete']} incomplete "
        f"sentences, {stats['short']} short gestures",
        file=sys.stderr,
    )


class ArrayReader:
    """Reads one JSON array element at a time from a text stream."""

    def __init__(self, stream, size=1 << 20):
        self.stream, self.size = stream, size
        self.decoder = json.JSONDecoder()
        self.buffer, self.pos, self.eof, self.started = "", 0, False, False

    def fill(self):
        chunk = self.stream.read(self.size)
        if not chunk:
            self.eof = True
            return
        if self.pos:
            self.buffer = self.buffer[self.pos:]
            self.pos = 0
        self.buffer += chunk

    def __iter__(self):
        while not self.started:
            while self.pos < len(self.buffer) and self.buffer[self.pos] in " \n\r\t":
                self.pos += 1
            if self.pos < len(self.buffer):
                if self.buffer[self.pos] != "[":
                    raise ValueError("expected a JSON array")
                self.pos += 1
                self.started = True
            elif self.eof:
                raise ValueError("unterminated JSON array")
            else:
                self.fill()
        while True:
            while self.pos < len(self.buffer) and self.buffer[self.pos] in " \n\r\t,":
                self.pos += 1
            if self.pos == len(self.buffer):
                if self.eof:
                    raise ValueError("unterminated JSON array")
                self.fill()
                continue
            if self.buffer[self.pos] == "]":
                return
            try:
                item, self.pos = self.decoder.raw_decode(self.buffer, self.pos)
            except json.JSONDecodeError:
                if self.eof:
                    raise
                self.fill()
                continue
            yield item


def linear(pairs):
    (x0, y0), (x1, y1) = pairs[0], pairs[-1]
    if abs(x1 - x0) < 1e-9:
        raise ValueError("degenerate fit")
    slope = (y1 - y0) / (x1 - x0)
    return slope, y0 - slope * x0


def keyboard_fit(keys):
    """Per-row x fits and one y fit from a keyboard's own key centres."""
    letters, rows = {}, []
    for row, name in enumerate(ROWS):
        found = []
        for key in keys:
            character = key["character"].lower()
            if len(character) == 1 and character in name:
                column = name.index(character)
                found.append((key, column))
                letters[character] = (row, column, key)
        if len(found) != len(name):
            raise ValueError(f"keyboard is missing keys from {name}")
        found.sort(key=lambda item: item[1])
        rows.append(linear([
            (key["x_center"], (column + 0.5 + ROW_OFFSETS[row]) / 10) for key, column in found
        ]))
    mean_y = [sum(letters[c][2]["y_center"] for c in name) / len(name) for name in ROWS]
    y_fit = linear(list(zip(mean_y, (1 / 6, 1 / 2, 5 / 6))))
    space = next((key for key in keys if key["character"].lower() == "<sp>"), None)
    return letters, rows, y_fit, space, keys


def nearest_key(x, y, key):
    dx = max(abs(x - key["x_center"]) - key["width"] / 2, 0)
    dy = max(abs(y - key["y_center"]) - key["height"] / 2, 0)
    return dx * dx + dy * dy


def noisy_tap(points, letters, rows, y_fit, space, keys):
    """Maps a tap's samples to the grid; says what it hit, or None for a key the format lacks."""
    first = points[0]
    key = min(keys, key=lambda candidate: nearest_key(first["x"], first["y"], candidate))
    if space is not None and key is space:
        left = space["x_center"] - space["width"] / 2
        return "space", [
            point(0.25 + 0.5 * min(1, max(0, (sample["x"] - left) / space["width"])), 1.25, sample["time"])
            for sample in points
        ]
    character = key["character"].lower()
    if len(character) != 1 or character not in letters:
        return None, None
    slope, offset = rows[letters[character][0]]
    slope_y, offset_y = y_fit
    return "letter", [
        # A letter stays out of the space-bar row the tap format marks with y > 1.
        point(slope * sample["x"] + offset, min(1, slope_y * sample["y"] + offset_y), sample["time"])
        for sample in points
    ]


def noisy_key(record):
    """One sentence in the plain and the force-aligned `_word` sets has the same key."""
    return (record["experiment"], record.get("condition"), record["participant"],
            record["set"].removesuffix("_word"), record.get("id"))


def noisy_words(archive, device):
    """The force-aligned `*_word` records' tap count for each of their words.

    The sentence-at-a-time logs of most experiments record no spaces at all, and
    the `swipe` condition draws a space as a right swipe. A `_word` record holds
    that attempt's letter taps split at the reference's words, so its lengths
    segment a sentence without guessing from its text.
    """
    words = {}
    with archive.open(next(n for n in archive.namelist() if n.endswith("noisy_typing.json"))) as raw:
        for record in ArrayReader(io.TextIOWrapper(raw, encoding="utf-8")):
            if record["device"] != device or not record["set"].endswith("_word"):
                continue
            if not record.get("ref"):
                continue
            counts = [sum(1 for entry in chunk["taps"] if entry["input"])
                      for chunk in record["chunks"] if not chunk["deleted"]]
            if len(counts) == len(record["ref"].split()):
                words[noisy_key(record)] = counts
    return words


def noisy_word_touches(counts, touches):
    """One word's taps at a time, with a space between words."""
    spaced, cursor = [], 0
    for index, count in enumerate(counts):
        word = touches[cursor:cursor + count]
        cursor += count
        if index and word:
            # A millisecond before the next word, never before the last letter.
            previous = spaced[-1][-1][2] if spaced else 0
            spaced.append([point(0.5, 1.25, max(previous, word[0][0][2] - 1))])
        spaced.extend(word)
    return spaced


def noisy(paths, writer, limit, device, wanted_set, word_sets, eyes_free, small_keyboards):
    stats = Counter()
    for path in map(Path, paths):
        if path.suffix != ".zip":
            raise SystemExit("noisy expects vertanen_noisy_typing_json.zip")
        with zipfile.ZipFile(path) as archive:
            keyboards = {k["keyboard"]: k["keys"] for k in json.load(
                archive.open(next(n for n in archive.namelist() if n.endswith("keyboards.json"))))}
            word_counts = noisy_words(archive, device)
            fits = {}
            with archive.open(next(n for n in archive.namelist() if n.endswith("noisy_typing.json"))) as raw:
                for record in ArrayReader(io.TextIOWrapper(raw, encoding="utf-8")):
                    if limited(writer.count, limit):
                        break
                    stats["records"] += 1
                    name = record["set"]
                    if record["device"] != device:
                        stats["device"] += 1
                        continue
                    # The eyes-free studies tapped a keyboard they imagined, and the
                    # small and tiny ones used 42 px and 26 px keys: neither is the
                    # full-size sighted typing the other datasets measure.
                    if not eyes_free and (record.get("condition") == "blindfolded" or (
                            record["experiment"].startswith("eyesfree")
                            and record.get("condition") != "sighted")):
                        stats["eyes_free"] += 1
                        continue
                    if not small_keyboards and (record.get("condition") in ("small", "tiny") or
                                                record["keyboard"].endswith(("_small", "_tiny"))):
                        stats["small_keyboard"] += 1
                        continue
                    if name.endswith("_word") != word_sets or (
                        wanted_set != "all" and name.removesuffix("_word") != wanted_set
                    ):
                        stats["sets"] += 1
                        continue
                    if not (record.get("ref") or "").split():
                        stats["noref"] += 1
                        continue
                    if record["keyboard"] not in fits:
                        fits[record["keyboard"]] = keyboard_fit(keyboards[record["keyboard"]])
                    letters, rows, y_fit, space, keys = fits[record["keyboard"]]
                    width = next(key["width"] for key in keys if key["character"].lower() == "q")
                    words = record["ref"].split()
                    touches, kinds = [], []
                    for chunk in record["chunks"]:
                        if chunk["deleted"]:
                            stats["deleted"] += 1
                            continue
                        for entry in chunk["taps"]:
                            if not entry["input"]:
                                stats["empty"] += 1
                                continue
                            kind, mapped = noisy_tap(entry["input"], letters, rows, y_fit, space, keys)
                            if kind is None:
                                stats["keys"] += 1
                                continue
                            span = max(sample["x"] for sample in entry["input"]) - min(
                                sample["x"] for sample in entry["input"])
                            touches.append(mapped)
                            kinds.append("wide" if kind == "letter" and span > 2 * width else kind)
                    if not touches:
                        stats["empty"] += 1
                        continue
                    letter_touches = [touch for touch, kind in zip(touches, kinds) if kind == "letter"]
                    counts = word_counts.get(noisy_key(record))
                    if (kinds.count("space") < len(words) - 1 and counts is not None
                            and len(counts) == len(words) and sum(counts) == len(letter_touches)):
                        touches = noisy_word_touches(counts, letter_touches)
                        stats["aligned"] += 1
                    elif "space" not in kinds and space is not None and "wide" in kinds:
                        # The study's right-swipe space: a touch that crossed two key widths.
                        touches = [
                            [point(0.5, 1.25, touch[0][2])] if kind == "wide" else touch
                            for touch, kind in zip(touches, kinds)
                        ]
                        stats["swiped"] += kinds.count("wide")
                    if sum(1 for touch in touches if touch[0][1] > 1) < len(words) - 1:
                        stats["run_on"] += 1
                    start = min((sample["time"] for chunk in record["chunks"]
                                 for entry in chunk["taps"] for sample in entry["input"]), default=0)
                    for touch in touches:
                        for index, (x, y, t) in enumerate(touch):
                            touch[index] = point(x, y, t - start)
                    writer.write(f"noisy-{device}-{name}", str(record["participant"]),
                                 words, "taps", [gesture(touch) for touch in touches])
                    stats["taps"] += len(touches)
                    stats["samples"] += sum(len(touch) for touch in touches)
    print(
        f"noisy: {writer.count} sentences, {stats['taps']} taps ({stats['samples']} samples); "
        f"skipped {stats['device']} other-device, {stats['eyes_free']} eyes-free, "
        f"{stats['small_keyboard']} small-keyboard, {stats['sets']} other-set, "
        f"{stats['deleted']} deleted chunks, {stats['keys']} non-letter taps, "
        f"{stats['noref']} without reference, {stats['empty']} empty; "
        f"{stats['aligned']} split by word, {stats['swiped']} with swipe spaces, "
        f"{stats['run_on']} run together",
        file=sys.stderr,
    )


def tsi_key(keys, x, y):
    def distance(key):
        dx = max(abs(x - key["key_center_x"]) - key["key_width"] / 2, 0)
        dy = max(abs(y - key["key_center_y"]) - key["key_height"] / 2, 0)
        return dx * dx + dy * dy

    return min(keys.values(), key=distance)


def tsi(paths, writer, limit, prompt_type):
    touch_path, prompt_path, keyboard_path = map(Path, paths)
    prompts = {}
    with open(prompt_path, newline="", encoding="utf-8") as file:
        for row in csv.DictReader(file):
            prompts[(row["participant_id"], row["task_id"], row["trial_id"])] = (
                row["prompt_type"], row["prompt"]
            )
    keys = json.loads(keyboard_path.read_text(encoding="utf-8"))["keys_info"]
    q, p, z = keys["q"], keys["p"], keys["z"]
    pitch_x = (p["key_center_x"] - q["key_center_x"]) / 9
    pitch_y = (z["key_center_y"] - q["key_center_y"]) / 2
    space = keys["SPACE"]

    stats = Counter()
    groups = OrderedDict()
    csv.field_size_limit(sys.maxsize)
    with open(touch_path, newline="", encoding="utf-8") as file:
        for row in csv.DictReader(file):
            stats["rows"] += 1
            key = (row["participant_id"], row["task_id"], row["trial_id"])
            if key not in prompts or (prompt_type == "phrase" and prompts[key][0] != "phrase"):
                stats["prompts"] += 1
                continue
            groups.setdefault(key, []).append({
                "time": int(row["timestamp_ms"]), "x": float(row["first_frame_touch_x"]),
                "y": float(row["first_frame_touch_y"]), "char": row["ref_char"],
                "deleted": row["was_deleted"].lower() == "true"})
    for key, rows in groups.items():
        if limited(writer.count, limit):
            break
        rows.sort(key=lambda row: row["time"])
        start = rows[0]["time"]
        touches = []
        for row in rows:
            if row["deleted"]:
                stats["deleted"] += 1
                continue
            hit = tsi_key(keys, row["x"], row["y"])
            if hit["key_id"] == "SPACE":
                left = space["key_center_x"] - space["key_width"] / 2
                x = 0.25 + 0.5 * min(1, max(0, (row["x"] - left) / space["key_width"]))
                touches.append(gesture([point(x, 1.25, row["time"] - start)]))
            elif hit["key_id"] == "." or row["char"] == ".":
                stats["period"] += 1
            elif len(hit["key_id"]) == 1 and hit["key_id"].isalpha():
                x = 0.05 + (row["x"] - q["key_center_x"]) / (10 * pitch_x)
                y = 1 / 6 + (row["y"] - q["key_center_y"]) / (3 * pitch_y)
                touches.append(gesture([point(x, y, row["time"] - start)]))
            else:
                stats["unknown"] += 1
        if not touches:
            stats["empty"] += 1
            continue
        writer.write("tsi", key[0], prompts[key][1].split(), "taps", touches)
        stats["taps"] += len(touches)
    print(
        f"tsi: {writer.count} sentences, {stats['taps']} taps; skipped {stats['deleted']} deleted "
        f"taps, {stats['period']} period taps, {stats['unknown']} other-key taps, "
        f"{stats['prompts']} non-phrase or unknown prompts, {stats['empty']} empty sentences; "
        f"{stats['rows']} rows",
        file=sys.stderr,
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="dataset", required=True)

    def common(command):
        command.add_argument("inputs", nargs="+"); command.add_argument("-o", "--output", required=True)
        command.add_argument("--limit", type=int, help="maximum sentences to emit")

    common(sub.add_parser("futo", help="FUTO swipe JSONL"))
    common(sub.add_parser("howweswipe", help="How We Swipe log, folder or swipelogs.zip"))
    noisy_parser = sub.add_parser("noisy", help="Noisy Typing JSON zip")
    common(noisy_parser)
    noisy_parser.add_argument("--device", default="phone")
    noisy_parser.add_argument("--set", dest="wanted_set", choices=("dev", "test", "all"), default="all")
    noisy_parser.add_argument("--word-sets", action="store_true", help="include *_word sets")
    noisy_parser.add_argument("--eyes-free", action="store_true",
                              help="include the eyes-free (blindfolded) studies")
    noisy_parser.add_argument("--small-keyboards", action="store_true",
                              help="include the small and tiny keyboard studies")
    tsi_parser = sub.add_parser("tsi", help="TSI touch_data.csv prompt_data.csv keyboard_data.json")
    common(tsi_parser)
    tsi_parser.add_argument("--prompt", dest="prompt_type", choices=("phrase", "all"), default="phrase")

    args = parser.parse_args()
    if args.dataset == "tsi" and len(args.inputs) != 3:
        parser.error("tsi needs touch_data.csv prompt_data.csv keyboard_data.json")
    writer = Writer(args.output)
    try:
        if args.dataset == "futo":
            futo(args.inputs, writer, args.limit)
        elif args.dataset == "howweswipe":
            howweswipe(args.inputs, writer, args.limit)
        elif args.dataset == "noisy":
            noisy(args.inputs, writer, args.limit, args.device, args.wanted_set, args.word_sets,
                  args.eyes_free, args.small_keyboards)
        else:
            tsi(args.inputs, writer, args.limit, args.prompt_type)
    finally:
        writer.close()


if __name__ == "__main__":
    main()
