#!/bin/sh
# Stage, test, and publish the AUR package hex-bin from a release's
# Linux executable. Runs on Arch (CI uses the archlinux container).
#
#   release-aur.sh stage   <version> <executable> <url> <dir>  # PKGBUILD, .SRCINFO, files
#   release-aur.sh test    <dir>                         # makepkg and install it
#   release-aur.sh publish <dir>                         # push to the AUR
#   release-aur.sh check                                 # the AUR accepts the key
#
# The package downloads the executable from <url>, the release service's
# linux-x64 file, so publish only after the release is published there. `publish` and `check` read the
# AUR SSH private key from AUR_SSH_PRIVATE_KEY.
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
package=hex-bin
usage() {
  echo "Usage: $0 stage <version> <executable> <url> <dir> | test <dir> | publish <dir> | check" >&2
  exit 1
}
[ $# -ge 1 ] || usage
mode=$1
shift

# Points GIT_SSH_COMMAND and $aur_ssh at the AUR with the CI key.
aur_ssh_setup() {
  : "${AUR_SSH_PRIVATE_KEY:?Set AUR_SSH_PRIVATE_KEY to the AUR SSH private key}"
  temporary=$(mktemp -d)
  trap 'rm -rf "$temporary"' EXIT HUP INT TERM
  (umask 077 && printf '%s\n' "$AUR_SSH_PRIVATE_KEY" > "$temporary/key")
  # aur.archlinux.org's published ED25519 host key.
  echo 'aur.archlinux.org ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIEuBKrPzbawxA/k2g6NcyV5jmqwJ2s+zpgZGZ7tpLIcN' > "$temporary/known_hosts"
  aur_ssh="ssh -i $temporary/key -o IdentitiesOnly=yes -o UserKnownHostsFile=$temporary/known_hosts -o StrictHostKeyChecking=yes"
  export GIT_SSH_COMMAND="$aur_ssh"
}

sha256_of() {
  sha256sum < "$1" | awk '{ print $1 }'
}

case "$mode" in
  stage)
    [ $# -eq 4 ] || usage
    version=$1 executable=$2 url=$3 dir=$4
    printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-(beta|dev)\.[0-9]+)?$'
    printf '%s\n' "$url" | grep -Eq '^https://(dev\.)?anoma\.ly/release/hex/files/[0-9a-f]{64}/hex-[0-9a-z.-]+-linux-x64$'
    # pkgver may not contain hyphens: a staging 2.4.0-dev.3 is 2.4.0_dev.3.
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
  publish)
    dir=$1
    version=$(sed -n 's/^pkgver=//p' "$dir/PKGBUILD")
    aur_ssh_setup
    git clone -q "ssh://aur@aur.archlinux.org/$package.git" "$temporary/aur"
    for file in PKGBUILD .SRCINFO hex.desktop hex.service LICENSE "$package.install"; do
      cp "$dir/$file" "$temporary/aur/$file"
    done
    cd "$temporary/aur"
    git add PKGBUILD .SRCINFO hex.desktop hex.service LICENSE "$package.install"
    if git diff --cached --quiet; then
      echo "The AUR already has $package $version." >&2
      exit 0
    fi
    git -c user.name='Hex release' -c user.email='hex@anoma.ly' commit -qm "Update to $version"
    git push -q origin HEAD:master
    echo "https://aur.archlinux.org/packages/$package"
    ;;
  check)
    aur_ssh_setup
    # list-repos succeeds only for a registered key; it names the account's packages.
    $aur_ssh aur@aur.archlinux.org list-repos
    ;;
  *) usage ;;
esac
