#!/usr/bin/env python3
import runpy
from pathlib import Path
import unittest
from unittest.mock import patch
import contextlib
import io
import json
import tempfile

score = runpy.run_path(str(Path(__file__).with_name("vocabulary-eval.py")))


class Scoring(unittest.TestCase):
    def test_regenerated_wav_replaces_old_receipt_and_limit_preserves_other_clips(self):
        generator = runpy.run_path(str(Path(__file__).with_name("vocabulary-corpus.py")))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture = Path(__file__).parent.parent / "tests/fixtures/vocabulary/manifest.json"
            prior = json.loads(fixture.read_text())
            prior["clips"] = prior["clips"][:2]
            prior["clips"][0]["sha256"] = "f" * 64
            (root / "manifest.json").write_text(json.dumps(prior))
            # No WAV exists: the first is regenerated; the second receipt must
            # survive this bounded generation pass. No provider call is made.
            with patch("sys.argv", ["generate", directory, "--generate", "--limit", "1"]), \
                 patch.dict("os.environ", {"ELEVENLABS_API_KEY": "test-only"}), \
                 patch("urllib.request.urlopen", return_value=io.BytesIO(b"\0\0" * 16000)), \
                 contextlib.redirect_stdout(io.StringIO()):
                generator["main"]()
            result = json.loads((root / "manifest.json").read_text())
            self.assertEqual(len(result["clips"]), 2)
            self.assertNotEqual(result["clips"][0]["sha256"], "f" * 64)
            self.assertEqual(result["clips"][1], prior["clips"][1])

    def test_boundaries_spacing_and_repetition(self):
        count = score["occurrences"]
        self.assertEqual(count("OPEN CODE and OpenCode", "OpenCode"), 2)
        self.assertEqual(count("hexadecimal toolkit effects runes", "Hex"), 0)
        self.assertEqual(count("hexadecimal toolkit effects runes", "Kit"), 0)
        self.assertEqual(count("A rune carved into stone", "Rune"), 1)

    def test_guard_and_target_denominators_and_all_hex_timings(self):
        manifest = dict(vocabulary=["Hex", "Rune"], clips=[
            dict(id="term", expected="Use Hex", expected_terms=["Hex"], group="target"),
            dict(id="guard", expected="Hanks found a rune", group="distractor")])
        results = [dict(type="clip", id="term", transcript="Use Hex", audio_ms=1000, inference_ms=10),
                   dict(type="clip", id="guard", transcript="Hex found a rune", audio_ms=1000, inference_ms=10)]
        for run, time in [(1, 10), (2, 20), (3, 60)]:
            results.extend(dict(type="measurement", id=c["id"], run=run, inference_ms=time) for c in manifest["clips"])
        report = score["score"](manifest, results)
        self.assertEqual(report["target"]["term_recall"], 1)
        self.assertIsNone(report["guard"]["term_recall"])
        self.assertEqual(report["guard"]["false_replacements"], 1)
        self.assertEqual(report["guard"]["false_replacement_rate"], 1)
        self.assertEqual(report["guard"]["wer"], .25)
        self.assertEqual(report["all"]["median_clip_ms"], 20)
        self.assertEqual(report["all"]["p95_clip_ms"], 60)
        with self.assertRaises(ValueError):
            score["score"](manifest, results[:-1])
        with self.assertRaises(ValueError):
            score["score"](manifest, results + [results[0]])


if __name__ == "__main__":
    unittest.main()
