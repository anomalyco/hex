#!/bin/sh
# Cut an App Store build: tag a dev commit ios-beta-appstore-<version>. Xcode Cloud builds
# it and ci_scripts/ci_post_clone.sh stamps <version> into project.yml for that build only.
set -eu

version="${1:-}"
commit="${2:-origin/dev}"
case "$version" in
  "" | *[!0-9.]* | .* | *.) echo "usage: $0 <version, e.g. 1.0.1> [commit]" >&2; exit 1 ;;
esac

git fetch -q origin dev --tags
sha=$(git rev-parse --verify "$commit^{commit}")
tag="ios-beta-appstore-$version"
if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
  echo "$tag already exists" >&2
  exit 1
fi
git tag -a "$tag" "$sha" -m "Hex Mobile $version App Store build"
git push -q origin "$tag"
echo "pushed $tag at $(git log --oneline -1 "$sha")"
