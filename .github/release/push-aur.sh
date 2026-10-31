#!/usr/bin/env bash
# push-aur.sh <dir> | check: pushes the staged hex-bin package (PKGBUILD,
# .SRCINFO, and its files, staged and tested by scripts/release-aur.sh in a
# job without secrets) to the AUR with AUR_SSH_PRIVATE_KEY, or with `check`
# confirms the AUR accepts the key.
. "$(dirname "$0")/lib.sh"

package=hex-bin
files=(PKGBUILD .SRCINFO anomaly.hex.desktop hex.service LICENSE "$package.install"
  anomaly.hex-{16,24,32,48,64,128,256,512}.png)
: "${AUR_SSH_PRIVATE_KEY:?Set AUR_SSH_PRIVATE_KEY to the AUR SSH private key}"
temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT
(umask 077 && printf '%s\n' "$AUR_SSH_PRIVATE_KEY" > "$temporary/key")
# aur.archlinux.org's published ED25519 host key.
echo 'aur.archlinux.org ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIEuBKrPzbawxA/k2g6NcyV5jmqwJ2s+zpgZGZ7tpLIcN' > "$temporary/known_hosts"
export GIT_SSH_COMMAND="ssh -i $temporary/key -o IdentitiesOnly=yes -o UserKnownHostsFile=$temporary/known_hosts -o StrictHostKeyChecking=yes"

if [ "${1:-}" = check ]; then
  # list-repos succeeds only for a registered key; it names the account's packages.
  $GIT_SSH_COMMAND aur@aur.archlinux.org list-repos
  exit 0
fi
dir=$(posix "${1:?Usage: push-aur.sh <dir> | check}")
for file in "${files[@]}"; do [ -f "$dir/$file" ] || die "The staged package lacks $file."; done
[ "$(find "$dir" -type f ! -name SHA256SUMS | wc -l | tr -d ' ')" = "${#files[@]}" ] || die "The staged package has unexpected files."
version=$(sed -n 's/^pkgver=//p' "$dir/PKGBUILD")
git clone -q "ssh://aur@aur.archlinux.org/$package.git" "$temporary/aur"
for file in "${files[@]}"; do cp "$dir/$file" "$temporary/aur/$file"; done
cd "$temporary/aur"
# Releases before the app id shipped the launcher entry as hex.desktop.
git rm -q --ignore-unmatch hex.desktop
git add "${files[@]}"
if git diff --cached --quiet; then
  echo "The AUR already has $package $version." >&2
  exit 0
fi
git -c user.name='Hex release' -c user.email='hex@anoma.ly' commit -qm "Update to $version"
git push -q origin HEAD:master
echo "https://aur.archlinux.org/packages/$package"
