#!/usr/bin/env bash
# push-homebrew.sh <cask> <version>: pushes the rendered cask (checked with
# brew in a job without secrets) to anomalyco/homebrew-tap as Casks/hex.rb
# with the tap deploy key, github-actions-hex-homebrew. A cask that already
# matches is left alone, so reruns are no-ops.
. "$(dirname "$0")/lib.sh"

cask=$(posix "$1")
version=$2
: "${HOMEBREW_TAP_DEPLOY_KEY:?Set HOMEBREW_TAP_DEPLOY_KEY to the tap deploy key}"
grep -q "^  version \"$version\"$" "$cask" || die "The cask is not version $version."
temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT
(umask 077 && printf '%s\n' "$HOMEBREW_TAP_DEPLOY_KEY" > "$temporary/key")
# github.com's published ED25519 host key.
echo 'github.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl' > "$temporary/known_hosts"
export GIT_SSH_COMMAND="ssh -i $temporary/key -o IdentitiesOnly=yes -o UserKnownHostsFile=$temporary/known_hosts -o StrictHostKeyChecking=yes"
git clone -q --depth 1 git@github.com:anomalyco/homebrew-tap.git "$temporary/tap"
cp "$cask" "$temporary/tap/Casks/hex.rb"
cd "$temporary/tap"
git add Casks/hex.rb
if git diff --cached --quiet; then
  echo "anomalyco/tap/hex already serves $version." >&2
  exit 0
fi
git -c user.name='Hex release' -c user.email='hex@anoma.ly' commit -qm "hex $version"
git push -q origin HEAD
echo "https://github.com/anomalyco/homebrew-tap/blob/$(git rev-parse HEAD)/Casks/hex.rb"
