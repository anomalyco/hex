#!/usr/bin/env python3
"""Fail when Cargo.lock's Git revisions and the Nix cargo pins disagree.

`nix/package.nix` records, for every Git dependency, the revision its
`cargoLock.outputHashes` entry was computed from. Bumping a pin in Cargo.lock
without updating that revision and hash makes `nix build` fail late with a
fixed-output hash mismatch. This check catches the revision half in a pull
request, before Nix downloads anything.

Cargo.lock writes Git sources as `git+URL[?query]#<commit>`, where the query
may name a branch, tag, or short rev, and `<commit>` is always the full
resolved commit. Only the fragment identifies the checked-out code, so the
query is ignored here. Python 3.9 is the floor: `scripts/check` runs with
Apple's interpreter on macOS.
"""

from __future__ import annotations

import argparse
from pathlib import Path
import re
import sys

REV = re.compile(r"[0-9a-f]{40}")
HASH = re.compile(r"sha256-[A-Za-z0-9+/]{43}=")
FAKE_HASH = re.compile(r"sha256-A{43}=")
PACKAGE_HEADER = "[[package]]"
TABLE_HEADER = re.compile(r"(?m)^\[")
NAME = re.compile(r'^name = "([^"]+)"$', re.MULTILINE)
VERSION = re.compile(r'^version = "([^"]+)"$', re.MULTILINE)
SOURCE = re.compile(r'^source = "([^"]+)"$', re.MULTILINE)
PIN_ENTRY = re.compile(r'"([^"]+)"\s*=\s*\{')
PIN_VALUE = re.compile(r'(rev|hash)\s*=\s*"([^"]+)";')


def fail(message: str) -> None:
    print(f"nix git pins: {message}", file=sys.stderr)
    sys.exit(1)


def parse_pins(path: Path) -> dict[str, dict[str, str]]:
    lines = path.read_text().splitlines()
    try:
        start = next(
            index
            for index, line in enumerate(lines)
            if line.strip() == "cargoGitDependencies = {"
        )
    except StopIteration:
        fail(f"{path} has no cargoGitDependencies table")

    pins: dict[str, dict[str, str]] = {}
    key = None
    for line in lines[start + 1 :]:
        stripped = line.strip()
        if stripped == "};":
            if key is not None:
                key = None
                continue
            break
        if not stripped or stripped.startswith("#"):
            continue
        entry = PIN_ENTRY.fullmatch(stripped)
        if entry:
            key = entry.group(1)
            if key in pins:
                fail(f"{path} declares {key} twice")
            pins[key] = {}
            continue
        value = PIN_VALUE.fullmatch(stripped)
        if value and key is not None:
            pins[key][value.group(1)] = value.group(2)
            continue
        fail(f"{path} has an unexpected line in cargoGitDependencies: {stripped}")

    if not pins:
        fail(f"{path} declares no Git dependencies")
    for name, pin in pins.items():
        if set(pin) != {"rev", "hash"}:
            fail(f"{path} needs one rev and one hash for {name}")
        if not REV.fullmatch(pin["rev"]):
            fail(f"{path} has no full 40-character rev for {name}")
        if FAKE_HASH.fullmatch(pin["hash"]):
            fail(f"{path} still has lib.fakeHash for {name}")
        if not HASH.fullmatch(pin["hash"]):
            fail(f"{path} has no sha256 output hash for {name}")
    return pins


def parse_git_source(source: str, package: str) -> tuple[str, str]:
    """Return the normalized repository URL and resolved commit in a source."""
    if "#" not in source:
        fail(
            f"Cargo.lock pins {package} to a Git source without a resolved "
            f"commit: {source}"
        )
    url_part, commit = source.rsplit("#", 1)
    if not REV.fullmatch(commit):
        fail(
            f"Cargo.lock pins {package} to a Git source without a full "
            f"40-character commit: {source}"
        )
    url = url_part[len("git+") :].split("?", 1)[0].rstrip("/")
    if url.endswith(".git"):
        url = url[:-4]
    if not url:
        fail(f"Cargo.lock pins {package} to an empty Git repository: {source}")
    return url, commit


def parse_lock(path: Path) -> dict[str, dict]:
    """One entry per Git repository: its resolved commit and its packages."""
    repos: dict[str, dict] = {}
    for stanza in path.read_text().split(PACKAGE_HEADER)[1:]:
        stanza = TABLE_HEADER.split(stanza, maxsplit=1)[0]
        name = NAME.search(stanza)
        version = VERSION.search(stanza)
        if name is None or version is None:
            fail(f"{path} has a package without a name or version")
        package = f"{name.group(1)}-{version.group(1)}"
        source = SOURCE.search(stanza)
        if source is None or not source.group(1).startswith("git+"):
            continue
        url, commit = parse_git_source(source.group(1), package)
        repo = repos.setdefault(url, {"rev": commit, "packages": set()})
        if repo["rev"] != commit:
            fail(f"Cargo.lock pins more than one commit of {url}")
        repo["packages"].add(package)
    return repos


def compare(pins: dict[str, dict[str, str]], repos: dict[str, dict]) -> list[str]:
    problems = []
    consumed = set()
    for url, repo in sorted(repos.items()):
        named = sorted(repo["packages"] & pins.keys())
        if not named:
            candidates = ", ".join(sorted(repo["packages"]))
            problems.append(
                f"{url}: Cargo.lock pins it, but the Nix table has no hash "
                f"(pin one of: {candidates})"
            )
            continue
        if len(named) > 1:
            problems.append(
                f"{url}: the Nix table names several of its packages: "
                + ", ".join(named)
            )
        consumed.update(named)
        for name in named:
            if pins[name]["rev"] != repo["rev"]:
                problems.append(
                    f"{name} ({url}): Cargo.lock names {repo['rev']}, "
                    f"the Nix table names {pins[name]['rev']}"
                )
    for name in sorted(pins.keys() - consumed):
        problems.append(f"{name}: the Nix table pins it, but Cargo.lock does not")
    return problems


def main() -> None:
    repo = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lock", type=Path, default=repo / "Cargo.lock")
    parser.add_argument("--pins", type=Path, default=repo / "nix/package.nix")
    args = parser.parse_args()

    pins = parse_pins(args.pins)
    repos = parse_lock(args.lock)
    problems = compare(pins, repos)

    if problems:
        print("nix git pins do not match Cargo.lock:", file=sys.stderr)
        for problem in problems:
            print(f"  {problem}", file=sys.stderr)
        fail(
            "update cargoGitDependencies in nix/package.nix with the new rev "
            "and the output hash from a failed `nix build .#hex`"
        )

    print(
        f"nix git pins match Cargo.lock "
        f"({len(repos)} Git repositories, {len(pins)} pins)"
    )


if __name__ == "__main__":
    main()
