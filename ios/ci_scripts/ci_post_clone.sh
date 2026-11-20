#!/bin/sh
set -eu

# The Xcode project is generated; the full dependency graph is checked in.
if [ -n "${CI_PRIMARY_REPOSITORY_PATH:-}" ]; then
  cd "$CI_PRIMARY_REPOSITORY_PATH/ios"
else
  cd "$(dirname "$0")/.."
fi
command -v xcodegen >/dev/null || brew install xcodegen
scripts/fetch-cleanup-model.sh
xcodegen generate
pins=HEXMobile.xcodeproj/project.xcworkspace/xcshareddata/swiftpm
mkdir -p "$pins"
cp Package.resolved "$pins/Package.resolved"
xcodebuild -resolvePackageDependencies -project HEXMobile.xcodeproj -scheme HEXMobile \
  -disableAutomaticPackageResolution -onlyUsePackageVersionsFromResolvedFile
