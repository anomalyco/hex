#!/usr/bin/env python3
"""Merge benchmark manifests or score Hex/decoder-spike JSONL without audio I/O."""

import argparse
import collections
import json
import hashlib
import math
from pathlib import Path
import re
import statistics


def words(text):
    return [w.lower() for w in re.findall(r"[^\W_]+", text)]


def occurrences(text, term):
    target = "".join(words(term))
    if not target:
        raise ValueError("empty vocabulary term")
    tokens = words(text)
    start, count = 0, 0
    while start < len(tokens):
        candidate = ""
        next_start = start + 1
        for end in range(start, len(tokens)):
            candidate += tokens[end]
            if candidate == target:
                count += 1
                next_start = end + 1
                break
            if not target.startswith(candidate):
                break
        start = next_start
    return count


def edits(reference, hypothesis):
    reference, hypothesis = words(reference), words(hypothesis)
    row = list(range(len(hypothesis) + 1))
    for i, left in enumerate(reference):
        current = [i + 1]
        for j, right in enumerate(hypothesis):
            current.append(min(row[j] + (left != right), row[j + 1] + 1, current[j] + 1))
        row = current
    return row[-1], len(reference)


def merge(output, inputs):
    clips, vocabulary, ids = [], {}, set()
    for source in inputs:
        manifest = json.loads(source.read_text())
        for term in manifest.get("vocabulary", []):
            vocabulary.setdefault("".join(words(term)), term)
        for clip in manifest["clips"]:
            if clip["id"] in ids:
                raise ValueError(f"duplicate clip: {clip['id']}")
            ids.add(clip["id"])
            audio = (source.parent / clip["audio"]).resolve(strict=True)
            if "sha256" in clip and hashlib.sha256(audio.read_bytes()).hexdigest() != clip["sha256"]:
                raise ValueError(f"WAV checksum mismatch: {clip['id']}")
            if "group" not in clip:
                raise ValueError(f"merge requires an explicit corpus group: {clip['id']}")
            clip["audio"] = str(audio)
            clips.append(clip)
    output.parent.mkdir(parents=True, exist_ok=True)
    # Merged manifests may contain private text and local absolute paths.
    import os
    with os.fdopen(os.open(output, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600), "w") as file:
        os.fchmod(file.fileno(), 0o600)
        json.dump(dict(vocabulary=list(vocabulary.values()), clips=clips), file, indent=2)
        file.write("\n")


def score(manifest, records):
    definitions = {c["id"]: c for c in manifest["clips"]}
    if len(definitions) != len(manifest["clips"]) or not definitions:
        raise ValueError("empty or duplicate clip IDs")
    runs = collections.defaultdict(dict)
    measurements = collections.defaultdict(dict)
    for row in records:
        if row["type"] == "measurement":
            if row["id"] not in definitions or row["id"] in measurements[row["run"]]:
                raise ValueError("unexpected/duplicate timing")
            measurements[row["run"]][row["id"]] = row
            continue
        if row["type"] != "clip":
            continue
        run = row.get("run", 1)
        if row["id"] not in definitions or row["id"] in runs[run]:
            raise ValueError(f"unexpected/duplicate result: {row['id']}")
        if "expected" in row and row["expected"] != definitions[row["id"]]["expected"]:
            raise ValueError(f"reference changed: {row['id']}")
        runs[run][row["id"]] = row
    if not runs or any(set(rows) != set(definitions) for rows in runs.values()):
        raise ValueError("incomplete run; refusing to score a partial corpus")
    first = runs[min(runs)]
    timings = runs
    if measurements:
        if any(set(rows) != set(definitions) for rows in measurements.values()):
            raise ValueError("incomplete timing pass")
        timings = {run: {ident: {**first[ident], **row} for ident, row in rows.items()}
                   for run, rows in measurements.items()}
    summaries = {}
    groups = sorted({c.get("group", "ordinary") for c in definitions.values()})
    for group in ["all", "guard"] + groups:
        subset = [c for c in definitions.values() if group == "all" or
                  (group == "guard" and c.get("group", "ordinary") in ("ordinary", "distractor")) or
                  group == c.get("group", "ordinary")]
        if not subset:
            continue
        errors = reference_words = expected = recalled = false = affected = 0
        for c in subset:
            text = first[c["id"]]["transcript"]
            e, n = edits(c["expected"], text)
            errors += e
            reference_words += n
            terms = c.get("expected_terms", [])
            expected += len(terms)
            recalled += sum(occurrences(text, t) > 0 for t in terms)
            inserted = sum(max(0, occurrences(text, t) - occurrences(c["expected"], t)) for t in manifest["vocabulary"])
            false += inserted
            affected += inserted > 0
        times = sorted(rows[c["id"]]["inference_ms"] for rows in timings.values() for c in subset)
        finish = sorted(rows[c["id"]]["finish_ms"] for rows in timings.values() for c in subset if "finish_ms" in rows[c["id"]])
        summaries[group] = dict(clips=len(subset), word_errors=errors, reference_words=reference_words,
            wer=errors / reference_words if reference_words else None, expected_terms=expected,
            recalled_terms=recalled, term_recall=recalled / expected if expected else None,
            false_replacements=false, false_replacement_rate=affected / len(subset) if manifest["vocabulary"] else None,
            median_clip_ms=statistics.median(times), p95_clip_ms=times[math.ceil(len(times)*.95)-1],
            p95_finish_ms=finish[math.ceil(len(finish)*.95)-1] if finish else None,
            median_rtf=statistics.median(sum(rows[c['id']]['inference_ms'] for c in subset) /
                sum(rows[c['id']]['audio_ms'] for c in subset) for rows in timings.values()))
    return summaries


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    merger = commands.add_parser("merge")
    merger.add_argument("output", type=Path)
    merger.add_argument("manifests", nargs="+", type=Path)
    scorer = commands.add_parser("score")
    scorer.add_argument("manifest", type=Path)
    scorer.add_argument("results", type=Path)
    args = parser.parse_args()
    if args.command == "merge":
        merge(args.output, args.manifests)
    else:
        records = [json.loads(line) for line in args.results.read_text().splitlines() if line.strip()]
        print(json.dumps(score(json.loads(args.manifest.read_text()), records), indent=2))


if __name__ == "__main__":
    main()
