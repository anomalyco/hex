#!/usr/bin/env python3
"""Fail when Cargo.lock's Git revisions and the Nix cargo pins disagree.

`nix/package.nix` records, for every Git dependency, the revision its
`cargoLock.outputHashes` entry was computed from. Bumping a pin in Cargo.lock
without updating that revision and hash makes `nix build` fail late with a
fixed-output hash mismatch. This check catches the revision half in a pull
request, before Nix downloads anything.
"""

from __future__ import annotations

import argparse
from pathlib import Path
import re
import sys
import tomllib

REV = re.compile(r"[0-9a-f]{40}")
HASH = re.compile(r"sha256-[A-Za-z0-9+/]{43}=")
CARGO_GIT_SOURCE = re.compile(r"^git\+(?P<url>[^?#]+)\?rev=(?P<rev>[0-9a-f]{40})#(?P<sha>[0-9a-f]{40})$")
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
        if not HASH.fullmatch(pin["hash"]):
            fail(f"{path} has no sha256 output hash for {name}")
    return pins


def parse_lock(path: Path) -> dict[str, dict]:
    """One entry per Git repository: its rev and the packages it provides."""
    lock = tomllib.loads(path.read_text())
    repos: dict[str, dict] = {}
    for package in lock.get("package", []):
        source = package.get("source", "")
        if not source.startswith("git+"):
            continue
        name = f"{package['name']}-{package['version']}"
        match = CARGO_GIT_SOURCE.fullmatch(source)
        if not match:
            fail(
                f"Cargo.lock pins {name} to a Git source without a full rev: {source}"
            )
        if match.group("rev") != match.group("sha"):
            fail(f"Cargo.lock names different revs for {name}: {source}")
        repo = repos.setdefault(
            match.group("url"), {"rev": match.group("rev"), "packages": set()}
        )
        if repo["rev"] != match.group("rev"):
            fail(f"Cargo.lock pins more than one rev of {match.group('url')}")
        repo["packages"].add(name)
    return repos


def main() -> None:
    repo = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lock", type=Path, default=repo / "Cargo.lock")
    parser.add_argument("--pins", type=Path, default=repo / "nix/package.nix")
    args = parser.parse_args()

    pins = parse_pins(args.pins)
    repos = parse_lock(args.lock)

    problems = []
    consumed = set()
    for url, repo in sorted(repos.items()):
        named = sorted(repo["packages"] & pins.keys())
        if not named:
            problem = f"{url}: Cargo.lock pins it, but the Nix table has no hash"
            if len(repo["packages"]) == 1:
                problem += f" (add {next(iter(repo['packages']))})"
            problems.append(problem)
            continue
        if len(named) > 1:
            problems.append(
                f"{url}: the Nix table names several of its packages: "
                + ", ".join(named)
            )
        name = named[0]
        consumed.add(name)
        if pins[name]["rev"] != repo["rev"]:
            problems.append(
                f"{name} ({url}): Cargo.lock names {repo['rev']}, "
                f"the Nix table names {pins[name]['rev']}"
            )
    for name in sorted(pins.keys() - consumed):
        problems.append(f"{name}: the Nix table pins it, but Cargo.lock does not")

    if problems:
        print("nix git pins do not match Cargo.lock:", file=sys.stderr)
        for problem in problems:
            print(f"  {problem}", file=sys.stderr)
        fail(
            "update cargoGitDependencies in nix/package.nix with the new rev "
            "and the output hash from a failed `nix build .#hex`"
        )

    print(f"nix git pins match Cargo.lock ({len(pins)} Git dependencies)")


if __name__ == "__main__":
    main()
