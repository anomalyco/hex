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
# The keyboard's Rust core (scripts/build-keyboard-core.sh) builds with rustup's cargo and
# the toolchain and iOS targets crates/keyboard pins (rust-toolchain.toml), in CARGO_HOME
# (by default ~/.cargo) as the script looks for it.
cargo_bin="${CARGO_HOME:-$HOME/.cargo}/bin"
if [ ! -x "$cargo_bin/rustup" ]; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
    sh -s -- -y -q --profile minimal --default-toolchain none --no-modify-path
fi
(cd ../crates/keyboard && "$cargo_bin/rustup" -q toolchain install)
xcodegen generate
pins=HEXMobile.xcodeproj/project.xcworkspace/xcshareddata/swiftpm
mkdir -p "$pins"
cp Package.resolved "$pins/Package.resolved"
xcodebuild -resolvePackageDependencies -project HEXMobile.xcodeproj -scheme HEXMobile \
  -disableAutomaticPackageResolution -onlyUsePackageVersionsFromResolvedFile
