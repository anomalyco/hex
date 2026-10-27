#!/bin/sh
# Stages and tests the AUR package hex-bin from a release's Linux executable,
# without secrets. Runs on Arch (CI uses the archlinux container). The
# Release workflow's AUR job pushes the staged files with
# .github/release/push-aur.sh, which holds the AUR key.
#
#   release-aur.sh stage <version> <executable> <url> <dir>  # PKGBUILD, .SRCINFO, files
#   release-aur.sh test  <dir>                               # makepkg and install it
#
# The package downloads the executable from <url>, the release service's
# linux-x64 file, so stage only after the release is published there.
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
package=hex-bin
usage() {
  echo "Usage: $0 stage <version> <executable> <url> <dir> | test <dir>" >&2
  exit 1
}
[ $# -ge 1 ] || usage
mode=$1
shift

sha256_of() {
  sha256sum < "$1" | awk '{ print $1 }'
}

case "$mode" in
  stage)
    [ $# -eq 4 ] || usage
    version=$1 executable=$2 url=$3 dir=$4
    printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-beta\.[0-9]+)?$'
    printf '%s\n' "$url" | grep -Eq '^https://anoma\.ly/release/hex/files/[0-9a-f]{64}/hex$'
    # pkgver may not contain hyphens: 2.4.0-beta.3 is 2.4.0_beta.3.
    pkgver=$(printf '%s' "$version" | tr - _)
    test "$(head -c 4 "$executable" | od -An -tx1 | tr -d ' ')" = 7f454c46
    mkdir -p "$dir"
    sed 's#@HEX_BIN@#/usr/bin/hex#g' "$root/packaging/hex.desktop" > "$dir/hex.desktop"
    cp "$root/packaging/aur/hex.service" "$root/packaging/aur/$package.install" "$dir/"
    cp "$root/LICENSE" "$dir/LICENSE"
    # makepkg uses a source already present under its file name, so `test`
    # downloads nothing.
    cp "$executable" "$dir/hex-$pkgver-linux-x64"
    sed -e "s/@VERSION@/$pkgver/" \
      -e "s#@URL@#$url#" \
      -e "s/@SHA256@/$(sha256_of "$executable")/" \
      -e "s/@DESKTOP_SHA256@/$(sha256_of "$dir/hex.desktop")/" \
      -e "s/@SERVICE_SHA256@/$(sha256_of "$dir/hex.service")/" \
      -e "s/@LICENSE_SHA256@/$(sha256_of "$dir/LICENSE")/" \
      "$root/packaging/aur/PKGBUILD" > "$dir/PKGBUILD"
    if grep -q '@[A-Z0-9_]*@' "$dir/PKGBUILD"; then
      echo "PKGBUILD has an unfilled placeholder." >&2
      exit 1
    fi
    (cd "$dir" && makepkg --printsrcinfo > .SRCINFO)
    echo "$dir"
    ;;
  test)
    dir=$1
    (cd "$dir" && makepkg --force --nodeps --noconfirm)
    built=$(cd "$dir" && makepkg --packagelist)
    test -f "$built"
    bsdtar -tf "$built" | grep -qx usr/bin/hex
    bsdtar -tf "$built" | grep -qx usr/lib/systemd/user/hex.service
    bsdtar -tf "$built" | grep -qx usr/share/applications/hex.desktop
    echo "$built"
    ;;
  *) usage ;;
esac
