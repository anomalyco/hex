"""Checks source file sizes across tracked Rust, TypeScript, and Swift files.

Rules:
  - Source files warn above 800 non-test lines and fail above 1,500 non-test lines
    (inline #[cfg(test)] items in Rust are excluded; test files warn above 1,000 lines).

Existing size ceilings live in scripts/file-size-exceptions.txt and only ratchet down.

Usage:
  python3 scripts/code-shape.py [--check] [--warn-only] [--update-exceptions [--allow-raise]]
"""

from __future__ import annotations

from dataclasses import dataclass
import os
import re
import subprocess
import sys

FILE_WARN = 800
FILE_FAIL = 1500
TEST_FILE = 1000

SIZE_EXC = "scripts/file-size-exceptions.txt"

SOURCE_RE = re.compile(r"\.(rs|tsx?|swift)$")
SKIPPED_RE = re.compile(
    r"(^|/)(node_modules|vendor|vendored|third_party|target|dist|build|\.build|out|"
    r"generated|__generated__|gen|fixtures?|Pods|DerivedData)/|"
    r"\.d\.ts$|\.(generated|gen|g|pb)\.(rs|tsx?|swift)$|_generated\.rs$"
)
TEST_RE = re.compile(
    r"(^|/)(tests?|benches|e2e|__tests__)/|(^|/)[^/]*Tests/|"
    r"(^|/)tests?\.rs$|_tests?\.rs$|\.(test|spec)\.tsx?$|Tests\.swift$"
)
CFG_TEST_RE = re.compile(r"^\s*#\[cfg\(test\)\]")
ATTR_RE = re.compile(r"^\s*#\[")
SEMI_END_RE = re.compile(r";\s*$")


@dataclass(frozen=True)
class Offender:
    path: str
    count: int
    fails: bool


@dataclass
class ShapeReport:
    sources: int
    files: list[Offender]
    tests: list[Offender]
    excepted: int


def is_source(path: str) -> bool:
    return bool(SOURCE_RE.search(path)) and not bool(SKIPPED_RE.search(path))


def is_test(path: str) -> bool:
    return bool(TEST_RE.search(path))


def braces(line: str) -> int:
    s = re.sub(r"\\.", "", line)
    s = re.sub(r'"[^"]*"', "", s)
    s = re.sub(r"'.'", "", s)
    s = re.sub(r"//.*", "", s)
    return s.count("{") - s.count("}")


def measure(path: str, text: str) -> tuple[int, int]:
    lines = [line.rstrip("\r") for line in text.split("\n")]
    if lines and lines[-1] == "":
        lines.pop()
    if is_test(path):
        return (0, len(lines))
    if not path.endswith(".rs"):
        return (len(lines), 0)

    code = 0
    state = 0
    depth = 0
    current = 0
    largest = 0

    for line in lines:
        if state == 0:
            if CFG_TEST_RE.match(line):
                state = 1
                current = 1
            else:
                code += 1
            continue

        current += 1
        if state == 1:
            if ATTR_RE.match(line):
                continue
            d = braces(line)
            if d > 0 or "{" in line:
                depth = d
                state = 2 if depth > 0 else 0
            elif SEMI_END_RE.search(line):
                state = 0
            if state == 0:
                largest = max(largest, current)
                current = 0
            continue

        depth += braces(line)
        if depth <= 0:
            state = 0
            largest = max(largest, current)
            current = 0

    largest = max(largest, current)
    return (code, largest)


def parse_exceptions(text: str) -> list[tuple[str, int | None]]:
    out: list[tuple[str, int | None]] = []
    for raw in text.split("\n"):
        cleaned = re.sub(r"#.*", "", raw).strip()
        if not cleaned:
            continue
        parts = cleaned.split()
        path = parts[0].rstrip("/")
        ceiling = int(parts[1]) if len(parts) > 1 and parts[1].isdigit() else None
        out.append((path, ceiling))
    return out


def sort_offenders(items: list[Offender]) -> list[Offender]:
    return sorted(items, key=lambda o: (-o.count, o.path))


def tracked_files(root: str) -> list[str]:
    proc = subprocess.run(
        ["git", "-C", root, "ls-files", "-z"],
        check=True,
        capture_output=True,
    )
    raw = proc.stdout.decode("utf-8", errors="replace").split("\0")
    return [
        p
        for p in raw
        if p and not (p.startswith("node_modules/") or "/node_modules/" in p)
    ]


def collect_raw_violations(
    files: list[str], lines_map: dict[str, tuple[int, int]]
) -> dict[str, int]:
    sources = [p for p in files if is_source(p)]
    return {
        p: lines_map[p][0]
        for p in sources
        if p in lines_map and lines_map[p][0] > FILE_FAIL
    }


def code_shape(
    files: list[str],
    lines_map: dict[str, tuple[int, int]],
    configs: dict[str, str],
) -> ShapeReport:
    sources = [p for p in files if is_source(p)]
    size_exc = dict(parse_exceptions(configs.get(SIZE_EXC, "")))
    excepted = 0

    def allowed(path: str, count: int) -> bool:
        nonlocal excepted
        if path not in size_exc:
            return False
        ceiling = size_exc[path]
        if ceiling is not None and count > ceiling:
            return False
        excepted += 1
        return True

    file_offenders: list[Offender] = []
    test_offenders: list[Offender] = []
    for path in sources:
        if path not in lines_map:
            continue
        code_lines, test_lines = lines_map[path]
        if code_lines > FILE_WARN:
            fails = code_lines > FILE_FAIL
            if not (fails and allowed(path, code_lines)):
                file_offenders.append(Offender(path, code_lines, fails))
        if test_lines > TEST_FILE:
            test_offenders.append(Offender(path, test_lines, False))

    return ShapeReport(
        sources=len(sources),
        files=sort_offenders(file_offenders),
        tests=sort_offenders(test_offenders),
        excepted=excepted,
    )


def fmt_int(n: int) -> str:
    return f"{n:,}"


def plural(count: int, one: str, many: str | None = None) -> str:
    return f"{count} {one if count == 1 else (many or one + 's')}"


def worst(items: list[Offender], render) -> str:
    head = ", ".join(render(o) for o in items[:2])
    tail = ", …" if len(items) > 2 else ""
    return f"({head}{tail})"


def hard_summary(report: ShapeReport) -> str | None:
    files = [o for o in report.files if o.fails]
    if not files:
        return None
    return (
        f"{plural(len(files), 'file')} over {fmt_int(FILE_FAIL)} lines "
        f"{worst(files, lambda o: f'{o.path} {fmt_int(o.count)}')}"
    )


def soft_summary(report: ShapeReport) -> str | None:
    files = sum(1 for o in report.files if not o.fails)
    parts: list[str] = []
    if files:
        parts.append(f"{plural(files, 'file')} over {FILE_WARN} lines")
    if report.tests:
        parts.append(
            f"{plural(len(report.tests), 'test file')} over {fmt_int(TEST_FILE)} lines"
        )
    return ", ".join(parts) if parts else None


def format_report(report: ShapeReport) -> str:
    hard = hard_summary(report)
    soft = soft_summary(report)
    headline = f"{plural(report.sources, 'source file')}: {hard or 'no hard problems'}"
    if soft:
        headline += f"; {soft}"
    if report.excepted:
        headline += f" ({report.excepted} excepted)"
    out = [headline]

    def section(title: str, items: list[Offender], unit: str) -> None:
        if not items:
            return
        out.append("")
        out.append(title)
        for o in items:
            mark = "✗" if o.fails else "!"
            out.append(f"  {mark} {fmt_int(o.count):>6} {unit}  {o.path}")

    section(
        f"Files (warn over {FILE_WARN} non-test lines, fail over {fmt_int(FILE_FAIL)})",
        report.files,
        "lines",
    )
    section(f"Tests (warn over {fmt_int(TEST_FILE)} lines)", report.tests, "lines")
    return "\n".join(out)


def write_ratcheted(
    exc_path: str,
    header_lines: list[str],
    current: dict[str, int],
    existing: dict[str, int | None],
    allow_raise: bool,
) -> int:
    new_entries: dict[str, int] = {}
    for key, count in current.items():
        if key not in existing:
            if allow_raise:
                new_entries[key] = count
            else:
                print(
                    f"not listing new violation {key} ({count}); use --allow-raise",
                    file=sys.stderr,
                )
        else:
            prev = existing[key]
            if prev is None or count <= prev or allow_raise:
                new_entries[key] = count
            else:
                new_entries[key] = prev

    with open(exc_path, "w", encoding="utf-8") as fh:
        for line in header_lines:
            fh.write(line + "\n")
        for key in sorted(new_entries):
            fh.write(f"{key} {new_entries[key]}\n")
    return len(new_entries)


def main(argv: list[str]) -> int:
    warn_only = False
    update = False
    allow_raise = False
    for arg in argv:
        if arg == "--check":
            pass
        elif arg == "--warn-only":
            warn_only = True
        elif arg == "--update-exceptions":
            update = True
        elif arg == "--allow-raise":
            allow_raise = True
        else:
            print(f"unknown option: {arg}", file=sys.stderr)
            return 2

    root = subprocess.check_output(
        ["git", "rev-parse", "--show-toplevel"], text=True
    ).strip()
    os.chdir(root)

    files = tracked_files(root)
    lines_map: dict[str, tuple[int, int]] = {}
    for path in files:
        if is_source(path) and os.path.exists(path):
            with open(path, encoding="utf-8", errors="replace") as fh:
                text = fh.read()
            lines_map[path] = measure(path, text)

    configs: dict[str, str] = {}
    if os.path.exists(SIZE_EXC):
        with open(SIZE_EXC, encoding="utf-8") as fh:
            configs[SIZE_EXC] = fh.read()

    if update:
        size_v = collect_raw_violations(files, lines_map)
        size_existing = dict(parse_exceptions(configs.get(SIZE_EXC, "")))
        n_size = write_ratcheted(
            SIZE_EXC,
            [
                f"# <path> <ceiling>: non-test line ceilings for files above {FILE_FAIL}. Ceilings only go down.",
                "# Refresh with python3 scripts/code-shape.py --update-exceptions.",
            ],
            size_v,
            size_existing,
            allow_raise,
        )
        print(f"wrote {SIZE_EXC} ({n_size} entries)")
        return 0

    report = code_shape(files, lines_map, configs)
    print(format_report(report))
    if warn_only or hard_summary(report) is None:
        return 0
    return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
