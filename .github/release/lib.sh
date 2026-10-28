# shellcheck shell=bash
# Shared helpers for the release scripts. Sourced, never executed.
#
# .github/release/ and .github/workflows/release.yml are the security
# boundary for releases (docs/releasing.md). Jobs that hold a secret or an
# OIDC token check out only this directory and run only these scripts, on
# verified artifacts that they treat as data: they never build, install
# dependencies, or execute repository code or artifact contents. Every value
# that names a destination comes from the environment the workflow sets.

set -euo pipefail

die() {
  echo "::error::$*" >&2
  exit 1
}

sha256() {
  if command -v sha256sum > /dev/null; then sha256sum "$@"; else shasum -a 256 "$@"; fi
}

# Windows runners pass D:\a\_temp style paths; Git Bash tools want /d/a/_temp.
posix() {
  if command -v cygpath > /dev/null; then cygpath -u "$1"; else printf '%s' "$1"; fi
}
