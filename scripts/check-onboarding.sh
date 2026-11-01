#!/usr/bin/env bash
# The onboarding check, run on the release candidate before every stable
# release (AGENTS.md › Releases; Release's onboarding job):
#
#   1. Renders every onboarding scene (fresh install, permissions, the model
#      step with nothing found, with found models, with the recommended model
#      already present, download in progress, practice, done) in both
#      appearances and compares each with its approved golden in
#      tests/golden/. Any difference fails and names the changed scenes; an
#      intentional change refreshes the goldens in the same PR
#      (HEX_UPDATE_GOLDENS=1). A GPU adapter is required (Mesa's lavapipe
#      will do), so the check never passes by skipping.
#   2. Walks a first run through the real tour in a clean temporary data
#      folder (fresh settings, no models, an empty Hugging Face cache): it
#      downloads and verifies the model the tour picks and reaches Done.
#
#   scripts/check-onboarding.sh [<out-dir>]   (default target/onboarding-check)
#
# <out-dir>/scenes/ holds every render, for the go/no-go message to Dax;
# <out-dir>/summary.md says what passed.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:-$root/target/onboarding-check}
mkdir -p "$out"
out=$(cd "$out" && pwd)
rm -rf "$out/scenes" "$out/summary.md"
mkdir -p "$out/scenes"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/hub"

cd "$root"
status=0
goldens=pass walkthrough=pass

echo "== Onboarding scenes against their goldens"
if ! HEX_REQUIRE_GPU=1 HEX_GOLDEN_RENDERS="$out/scenes" \
  cargo test --locked --bin hex -- --exact rune_ui::tests::onboarding_check::onboarding_goldens; then
  goldens=fail status=1
fi

echo "== First run in a clean data folder"
if ! env -u HF_HOME HEX_APPLICATION_SUPPORT_DIR="$work/support" HF_HUB_CACHE="$work/hub" \
  cargo test --locked --bin hex -- --exact --ignored --nocapture \
  rune_ui::tests::onboarding_check::onboarding_walkthrough; then
  walkthrough=fail status=1
fi

{
  echo "### Onboarding check: $([ "$status" = 0 ] && echo pass || echo FAIL)"
  echo
  echo "- Commit: $(git rev-parse HEAD)"
  echo "- Scenes against goldens: $goldens ($(find "$out/scenes" -name '*.png' | wc -l | tr -d ' ') renders)"
  echo "- First run in a clean data folder: $walkthrough"
} > "$out/summary.md"
cat "$out/summary.md"
exit "$status"
