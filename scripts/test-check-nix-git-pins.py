#!/usr/bin/env python3
"""Tests for scripts/check-nix-git-pins.py. Python 3.9 compatible."""

import importlib.util
import io
from contextlib import redirect_stderr
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("check-nix-git-pins.py")
SPEC = importlib.util.spec_from_file_location("check_nix_git_pins", SCRIPT)
check = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check)

SHA_A = "a" * 40
SHA_B = "b" * 40
VALID_HASH = "sha256-" + "B" * 43 + "="
FAKE_HASH = "sha256-" + "A" * 43 + "="


def lock_text(packages):
    stanzas = ["version = 4"]
    for name, version, source in packages:
        stanzas.append(
            "\n".join(
                [
                    "[[package]]",
                    'name = "{}"'.format(name),
                    'version = "{}"'.format(version),
                    'source = "{}"'.format(source),
                    "dependencies = [",
                    ' "a-dependency-without-a-name-field",',
                    "]",
                ]
            )
        )
    return "\n\n".join(stanzas) + "\n"


def pins_text(entries):
    lines = ["let", "  cargoGitDependencies = {"]
    for name, rev, digest in entries:
        lines.extend(
            [
                '    "{}" = {{'.format(name),
                '      rev = "{}";'.format(rev),
                '      hash = "{}";'.format(digest),
                "    };",
            ]
        )
    lines.extend(["  };", "in", "{}"])
    return "\n".join(lines) + "\n"


def write(directory, name, text):
    path = Path(directory) / name
    path.write_text(text)
    return path


def run_cli(lock, pins):
    with tempfile.TemporaryDirectory(prefix="hex-nix-pins-") as directory:
        return subprocess.run(
            [
                sys.executable,
                str(SCRIPT),
                "--lock",
                str(write(directory, "Cargo.lock", lock)),
                "--pins",
                str(write(directory, "package.nix", pins)),
            ],
            capture_output=True,
            text=True,
        )


class ParseGitSourceTest(unittest.TestCase):
    def test_each_cargo_source_form(self):
        cases = [
            (
                "branch with .git",
                "git+https://github.com/example/branch.git?branch=main#" + SHA_A,
                "https://github.com/example/branch",
                SHA_A,
            ),
            (
                "tag",
                "git+https://github.com/example/tag?tag=v1.2.3#" + SHA_B,
                "https://github.com/example/tag",
                SHA_B,
            ),
            (
                "short rev",
                "git+https://github.com/example/short?rev=abc1234#" + SHA_A,
                "https://github.com/example/short",
                SHA_A,
            ),
            (
                "bare fragment",
                "git+https://github.com/example/bare#" + SHA_B,
                "https://github.com/example/bare",
                SHA_B,
            ),
            (
                "full rev",
                "git+https://github.com/example/full?rev=" + SHA_A + "#" + SHA_A,
                "https://github.com/example/full",
                SHA_A,
            ),
        ]
        for label, source, url, commit in cases:
            with self.subTest(label):
                self.assertEqual(check.parse_git_source(source, "pkg-0.0.0"), (url, commit))

    def test_rejects_source_without_a_full_fragment(self):
        sources = [
            "git+https://github.com/example/repo?rev=" + SHA_A,
            "git+https://github.com/example/repo#abc1234",
            "git+https://github.com/example/repo#",
        ]
        for source in sources:
            with self.subTest(source):
                with self.assertRaises(SystemExit), redirect_stderr(io.StringIO()):
                    check.parse_git_source(source, "pkg-0.0.0")


class ParseLockTest(unittest.TestCase):
    def parse(self, text):
        with tempfile.TemporaryDirectory(prefix="hex-nix-pins-") as directory:
            return check.parse_lock(write(directory, "Cargo.lock", text))

    def test_every_source_form_and_dot_git_grouping(self):
        repos = self.parse(
            lock_text(
                [
                    (
                        "branch",
                        "0.1.0",
                        "git+https://github.com/example/branch.git?branch=main#"
                        + SHA_A,
                    ),
                    (
                        "tag",
                        "0.1.0",
                        "git+https://github.com/example/tag?tag=v1.2.3#" + SHA_B,
                    ),
                    (
                        "short",
                        "0.1.0",
                        "git+https://github.com/example/short?rev=abc1234#" + SHA_A,
                    ),
                    ("bare", "0.1.0", "git+https://github.com/example/bare#" + SHA_B),
                    (
                        "full",
                        "0.1.0",
                        "git+https://github.com/example/full?rev=" + SHA_A + "#" + SHA_A,
                    ),
                    (
                        "plain-one",
                        "0.1.0",
                        "git+https://github.com/example/plain?rev=abc1234#" + SHA_A,
                    ),
                    (
                        "plain-two",
                        "0.2.0",
                        "git+https://github.com/example/plain.git?rev=abc1234#"
                        + SHA_A,
                    ),
                    (
                        "registry",
                        "0.1.0",
                        "registry+https://github.com/rust-lang/crates.io-index",
                    ),
                ]
            )
        )
        packages = set()
        for repo in repos.values():
            packages.update(repo["packages"])
        self.assertNotIn("registry-0.1.0", packages)
        self.assertEqual(
            repos["https://github.com/example/plain"]["packages"],
            {"plain-one-0.1.0", "plain-two-0.2.0"},
        )
        self.assertEqual(repos["https://github.com/example/branch"]["rev"], SHA_A)
        self.assertEqual(repos["https://github.com/example/tag"]["rev"], SHA_B)
        self.assertEqual(repos["https://github.com/example/short"]["rev"], SHA_A)
        self.assertEqual(repos["https://github.com/example/bare"]["rev"], SHA_B)
        self.assertEqual(repos["https://github.com/example/full"]["rev"], SHA_A)
        self.assertEqual(len(repos), 6)

    def test_sourceless_package_is_ignored(self):
        lock = """version = 4

[[package]]
name = "hex"
version = "3.3.0"
dependencies = []
"""
        self.assertEqual(self.parse(lock), {})

    def test_trailing_patch_table_is_not_attributed(self):
        lock = f"""version = 4

[[package]]
name = "hex"
version = "3.3.0"
dependencies = [
 "rune",
]

[[patch.unused]]
name = "rune"
version = "0.0.1"
source = "git+https://github.com/example/rune?branch=main#{SHA_A}"
"""
        self.assertEqual(self.parse(lock), {})


class CompareTest(unittest.TestCase):
    def test_one_pin_covers_a_multi_crate_repo(self):
        pins = {"rune-0.0.1": {"rev": SHA_A, "hash": VALID_HASH}}
        repos = {
            "https://github.com/example/rune": {
                "rev": SHA_A,
                "packages": {"rune-0.0.1", "rune-kit-0.0.1"},
            }
        }
        self.assertEqual(check.compare(pins, repos), [])

    def test_missing_multi_crate_repo_lists_every_candidate(self):
        problems = check.compare(
            {},
            {
                "https://github.com/example/rune": {
                    "rev": SHA_A,
                    "packages": {"rune-0.0.1", "rune-kit-0.0.1"},
                }
            },
        )
        self.assertEqual(len(problems), 1)
        self.assertIn("rune-0.0.1", problems[0])
        self.assertIn("rune-kit-0.0.1", problems[0])

    def test_several_pins_do_not_report_an_unused_pin(self):
        pins = {
            "rune-0.0.1": {"rev": SHA_A, "hash": VALID_HASH},
            "rune-kit-0.0.1": {"rev": SHA_A, "hash": VALID_HASH},
        }
        problems = check.compare(
            pins,
            {
                "https://github.com/example/rune": {
                    "rev": SHA_A,
                    "packages": {"rune-0.0.1", "rune-kit-0.0.1"},
                }
            },
        )
        self.assertEqual(
            problems,
            [
                "https://github.com/example/rune: the Nix table names several "
                "of its packages: rune-0.0.1, rune-kit-0.0.1"
            ],
        )

    def test_stale_revision(self):
        pins = {"rune-0.0.1": {"rev": SHA_B, "hash": VALID_HASH}}
        problems = check.compare(
            pins,
            {
                "https://github.com/example/rune": {
                    "rev": SHA_A,
                    "packages": {"rune-0.0.1"},
                }
            },
        )
        self.assertEqual(len(problems), 1)
        self.assertIn(SHA_A, problems[0])
        self.assertIn(SHA_B, problems[0])


class ParsePinsTest(unittest.TestCase):
    def parse(self, text):
        with tempfile.TemporaryDirectory(prefix="hex-nix-pins-") as directory:
            return check.parse_pins(write(directory, "package.nix", text))

    def test_accepts_a_valid_pin(self):
        pins = self.parse(pins_text([("rune-0.0.1", SHA_A, VALID_HASH)]))
        self.assertEqual(pins["rune-0.0.1"]["rev"], SHA_A)

    def test_rejects_lib_fake_hash(self):
        stderr = io.StringIO()
        with self.assertRaises(SystemExit), redirect_stderr(stderr):
            self.parse(pins_text([("rune-0.0.1", SHA_A, FAKE_HASH)]))
        self.assertIn("fakeHash", stderr.getvalue())


class CliTest(unittest.TestCase):
    def test_passes_with_one_pin_for_a_multi_crate_repo(self):
        lock = lock_text(
            [
                (
                    "rune",
                    "0.0.1",
                    "git+https://github.com/example/rune?rev=" + SHA_A + "#" + SHA_A,
                ),
                (
                    "rune-kit",
                    "0.0.1",
                    "git+https://github.com/example/rune?rev=" + SHA_A + "#" + SHA_A,
                ),
            ]
        )
        result = run_cli(lock, pins_text([("rune-0.0.1", SHA_A, VALID_HASH)]))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("match", result.stdout)

    def test_fails_on_a_stale_revision(self):
        lock = lock_text(
            [
                (
                    "rune",
                    "0.0.1",
                    "git+https://github.com/example/rune?rev=" + SHA_A + "#" + SHA_A,
                ),
            ]
        )
        result = run_cli(lock, pins_text([("rune-0.0.1", SHA_B, VALID_HASH)]))
        self.assertEqual(result.returncode, 1)
        self.assertIn("Cargo.lock names", result.stderr)

    def test_fails_on_lib_fake_hash(self):
        lock = lock_text(
            [
                (
                    "rune",
                    "0.0.1",
                    "git+https://github.com/example/rune?rev=" + SHA_A + "#" + SHA_A,
                ),
            ]
        )
        result = run_cli(lock, pins_text([("rune-0.0.1", SHA_A, FAKE_HASH)]))
        self.assertEqual(result.returncode, 1)
        self.assertIn("fakeHash", result.stderr)


if __name__ == "__main__":
    unittest.main()
