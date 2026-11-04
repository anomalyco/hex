#!/usr/bin/env bash
# check-glibc-floor.sh <executable>: fails when a Linux release executable
# needs a newer glibc or libstdc++ than Debian 12 ships (glibc 2.36, GCC 12's
# GLIBCXX_3.4.30), so it keeps running on Debian 12, Ubuntu 23.04, Fedora 37,
# and newer (issue #1026). packaging/linux/build.Dockerfile sets the floor;
# raise both together.
. "$(dirname "$0")/lib.sh"

glibc_floor=2.36
glibcxx_floor=3.4.30

[ $# -eq 1 ] || die "Usage: check-glibc-floor.sh <executable>"
[ -f "$1" ] || die "$1 does not exist."
symbols=$(objdump -T "$1") || die "objdump could not read $1."

# highest <prefix>: the newest version of a symbol family the executable needs.
highest() { { grep -o "$1[0-9.]*[0-9]" <<< "$symbols" || true; } | sed "s/^$1//" | sort -uV | tail -1; }
# needing <symbol version>: up to five symbols that need it.
needing() { awk -v v="($1)" 'index($0, v) { print $NF }' <<< "$symbols" | head -5 | tr '\n' ' '; }
# newer <version> <floor>: true when version is past floor.
newer() { [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -1)" != "$2" ]; }

glibc=$(highest GLIBC_)
glibcxx=$(highest GLIBCXX_)
[ -n "$glibc" ] || die "$1 has no GLIBC symbol versions; is it a dynamically linked glibc executable?"
if newer "$glibc" "$glibc_floor"; then
  die "$(basename "$1") needs GLIBC_$glibc, newer than the GLIBC_$glibc_floor floor: $(needing "GLIBC_$glibc")"
fi
if [ -n "$glibcxx" ] && newer "$glibcxx" "$glibcxx_floor"; then
  die "$(basename "$1") needs GLIBCXX_$glibcxx, newer than the GLIBCXX_$glibcxx_floor floor: $(needing "GLIBCXX_$glibcxx")"
fi
echo "$(basename "$1") needs GLIBC_$glibc${glibcxx:+ and GLIBCXX_$glibcxx} (floor GLIBC_$glibc_floor, GLIBCXX_$glibcxx_floor)."
