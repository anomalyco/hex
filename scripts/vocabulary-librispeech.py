#!/usr/bin/env python3
"""Fetch 200 deterministic LibriSpeech test-clean guard clips outside git.

Streams the public archive, retaining only the first 200 FLAC members and their
chapter transcripts; stops once their references are available. Requires ffmpeg.
LibriSpeech: https://www.openslr.org/12 (CC BY 4.0).
"""

import argparse
import hashlib
import json
import runpy
from pathlib import Path
import subprocess
import tarfile
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    root = args.output
    (root / "audio").mkdir(parents=True, exist_ok=True)
    url = "https://www.openslr.org/resources/12/test-clean.tar.gz"
    references = {}
    clips = []
    with urllib.request.urlopen(url, timeout=120) as response:
        with tarfile.open(fileobj=response, mode="r|gz") as archive:
            for member in archive:
                name = Path(member.name).name
                if not member.isfile():
                    continue
                if name.endswith(".trans.txt"):
                    for line in archive.extractfile(member).read().decode().splitlines():
                        ident, text = line.split(" ", 1)
                        references[ident] = text
                elif name.endswith(".flac") and len(clips) < 200:
                    ident = Path(name).stem
                    flac = archive.extractfile(member).read()
                    wav = root / "audio" / f"{ident}.wav"
                    subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", "pipe:0", "-ar", "16000",
                                    "-ac", "1", "-c:a", "pcm_s16le", "-bitexact", "-map_metadata", "-1", str(wav)], input=flac, check=True)
                    clips.append(dict(id=ident, audio=f"audio/{ident}.wav", group="ordinary",
                                      expected_terms=[], sha256=hashlib.sha256(wav.read_bytes()).hexdigest()))
                if len(clips) == 200 and all(c["id"] in references for c in clips):
                    break
    if len(clips) != 200 or any(c["id"] not in references for c in clips):
        raise ValueError("archive did not contain 200 paired clips")
    for clip in clips:
        clip["expected"] = references[clip["id"]]
    corpus = runpy.run_path(str(Path(__file__).with_name("vocabulary-corpus.py")))
    manifest = dict(vocabulary=corpus["TERMS"], provenance=dict(url=url, license="CC-BY-4.0",
                    selection="first 200 FLAC members in archive order; not speaker-balanced"), clips=clips)
    (root / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"Saved {len(clips)} ordinary-speech clips to {root}")


if __name__ == "__main__":
    main()
