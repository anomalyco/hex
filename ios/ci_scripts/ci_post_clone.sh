#!/bin/sh
set -eu

# The Xcode project is generated; the full dependency graph is checked in.
if [ -n "${CI_PRIMARY_REPOSITORY_PATH:-}" ]; then
  cd "$CI_PRIMARY_REPOSITORY_PATH/ios"
else
  cd "$(dirname "$0")/.."
fi
command -v xcodegen >/dev/null || brew install xcodegen
# An App Store build is tagged ios-beta-appstore-<version> (e.g. ios-beta-appstore-0.2.1):
# it is stamped with that version, while the repo and TestFlight stay on project.yml's.
case "${CI_TAG:-}" in
  ios-beta-appstore-*)
    version="${CI_TAG#ios-beta-appstore-}"
    case "$version" in
      *[!0-9.]* | "" | .* | *.) echo "Bad App Store version in tag: $CI_TAG" >&2; exit 1 ;;
    esac
    sed -i '' -E "s/^( +MARKETING_VERSION: ).*/\1$version/" project.yml
    grep -n 'MARKETING_VERSION' project.yml
    ;;
esac
scripts/fetch-cleanup-model.sh
# The keyboard's Rust core (scripts/build-keyboard-core.sh) builds with rustup and the
# toolchain crates/keyboard pins (rust-toolchain.toml). Without a rustup the build script
# would find (crates/keyboard/tools/rustup-env.sh), install one: rustup-init 1.29.3 from its
# versioned URL, checked against the SHA-256 static.rust-lang.org publishes beside it, into
# CARGO_HOME (~/.cargo by default).
. ../crates/keyboard/tools/rustup-env.sh
if ! find_rustup ../crates/keyboard; then
  case "$(uname -m)" in
    arm64) host=aarch64-apple-darwin sha256=63752a1919a3a4793173ec70f8f212621a7ce38ba6a246095984482f4de20f9f ;;
    x86_64) host=x86_64-apple-darwin sha256=bc107f1fb0228de7fa32413c89c1b305cea9cd256e94b01150cd054128145429 ;;
    *) echo "No rustup-init for $(uname -m)" >&2; exit 1 ;;
  esac
  init="$(mktemp -d)/rustup-init"
  curl --proto '=https' --tlsv1.2 -sSf -o "$init" "https://static.rust-lang.org/rustup/archive/1.29.3/$host/rustup-init"
  echo "$sha256  $init" | shasum -a 256 -c - >/dev/null
  chmod +x "$init"
  "$init" -y -q --profile minimal --default-toolchain none --no-modify-path
  (cd ../crates/keyboard && "${CARGO_HOME:-$HOME/.cargo}/bin/rustup" -q toolchain install)
  find_rustup ../crates/keyboard || { echo "rustup-init left no rustup where the build looks for it" >&2; exit 1; }
fi
(cd ../crates/keyboard && "$rustup_bin/rustup" -q toolchain install)
xcodegen generate
pins=HEXMobile.xcodeproj/project.xcworkspace/xcshareddata/swiftpm
mkdir -p "$pins"
cp Package.resolved "$pins/Package.resolved"
xcodebuild -resolvePackageDependencies -project HEXMobile.xcodeproj -scheme HEXMobile \
  -disableAutomaticPackageResolution -onlyUsePackageVersionsFromResolvedFile
