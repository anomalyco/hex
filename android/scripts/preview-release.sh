#!/usr/bin/env bash
# Publishes one Hex Preview build: the preview APK (outside Google Play, ly.anoma.hex.preview) built from one
# commit of dev on a build Mac, as its own GitHub pre-release with its own tag at that commit, then points the
# channel (a branch holding only latest.json and README.md) at it. Hex Preview reads that latest.json to update.
#
#   android/scripts/preview-release.sh --notes notes.txt [--commit <sha>]          the real channel
#   android/scripts/preview-release.sh --repo you/fork --channel android-preview-test --commit <sha> --notes notes.txt
#
# --commit   what to build (default: origin/dev's head). The real channel takes only commits on dev.
# --repo     where the release and the channel live (default anomalyco/hex). A test channel is a fork.
# --channel  the channel branch (default android-preview). The APK is built to read this branch's latest.json.
# --notes    what changed, one short line each: the app shows them above Update, and the release body lists them.
# --builder  the ferry machine with the Android SDK, NDK and JDK 21 (default anomastudio; android/README.md).
#
# Build numbers are the count of commits up to the built commit (git rev-list --count), so they only grow along
# dev; a build whose tag exists, or that isn't newer than the channel's, is refused. Tags are never moved or reused.
# Every build is signed with the Hex Preview key: Android refuses an update signed with any other. The key and its
# keystore.properties stay outside the repository, in ~/.config/hex-android-preview on this Mac and the builder.
set -euo pipefail
# ferry's connection to a busy builder can drop; every step it retries is safe to repeat.
retry() { for _ in 1 2 3 4; do "$@" && return 0; sleep 5; done; "$@"; }


repo=anomalyco/hex channel=android-preview commit="" notes="" builder=anomastudio
while [ $# -gt 0 ]; do
  case "$1" in
    --repo) repo=$2; shift 2 ;;
    --channel) channel=$2; shift 2 ;;
    --commit) commit=$2; shift 2 ;;
    --notes) notes=$2; shift 2 ;;
    --builder) builder=$2; shift 2 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done
# The Hex Preview certificate's SHA-256 (the key itself never leaves ~/.config/hex-android-preview).
fingerprint=133568F6F0614FE57BC88DAECA32AFA72C6AB740202629F752FAA033BE35D98E
root=$(git rev-parse --show-toplevel)
key=~/.config/hex-android-preview
real=$([ "$repo" = anomalyco/hex ] && [ "$channel" = android-preview ] && echo yes || echo no)

[ -n "$notes" ] && [ -s "$notes" ] || { echo "--notes <file> with one line per change is required" >&2; exit 2; }
[ -f "$key/keystore.properties" ] && [ -f "$key/preview.jks" ] || { echo "missing $key (the Hex Preview key)" >&2; exit 1; }
git -C "$root" fetch -q origin dev
commit=$(git -C "$root" rev-parse --verify "${commit:-origin/dev}^{commit}")
if [ "$real" = yes ] && ! git -C "$root" merge-base --is-ancestor "$commit" origin/dev; then
  echo "$commit is not on dev; the real channel takes only dev's commits" >&2; exit 1
fi
build=$(git -C "$root" rev-list --count "$commit")
tag=android-preview-$build
version=$(git -C "$root" show "$commit:android/app/build.gradle.kts" | sed -n 's/^ *versionName = "\(.*\)"$/\1/p' | head -1)
minsdk=$(git -C "$root" show "$commit:android/app/build.gradle.kts" | sed -n 's/^ *minSdk = \([0-9]*\)$/\1/p' | head -1)
name=$version-preview.$build
raw=https://raw.githubusercontent.com/$repo/$channel/latest.json
echo "Hex Preview $name from ${commit:0:12} -> $repo $tag, channel $channel"

if gh api "repos/$repo/git/ref/tags/$tag" >/dev/null 2>&1; then echo "tag $tag already exists; tags are never reused" >&2; exit 1; fi
current=$(curl -fsSL "https://api.github.com/repos/$repo/contents/latest.json?ref=$channel" -H 'Accept: application/vnd.github.raw' 2>/dev/null \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["versionCode"])' 2>/dev/null || echo 0)
[ "$build" -gt "$current" ] || { echo "build $build is not newer than the channel's $current" >&2; exit 1; }

# Build on the builder from exactly that commit's android/ folder.
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
git -C "$root" archive "$commit" android | tar -x -C "$work"
remote=dev/hex-preview-build/$build
retry ferry sh "$builder" "rm -rf ~/$remote && mkdir -p ~/$remote ~/.config/hex-android-preview && chmod 700 ~/.config/hex-android-preview"
ferry sh "$builder" '[ -f ~/.config/hex-android-preview/preview.jks ] && [ -f ~/.config/hex-android-preview/keystore.properties ]' || {
  retry ferry cp "$key/preview.jks" "$builder:.config/hex-android-preview/preview.jks" >/dev/null
  retry ferry cp "$key/keystore.properties" "$builder:.config/hex-android-preview/keystore.properties" >/dev/null
  retry ferry sh "$builder" 'chmod 600 ~/.config/hex-android-preview/*'
}
retry ferry cp "$work/android" "$builder:$remote/" >/dev/null
jdk='export JAVA_HOME=/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home ANDROID_HOME=$HOME/Library/Android/sdk; export PATH=$JAVA_HOME/bin:$PATH;'
# Detached, so a dropped connection to the builder doesn't stop the build; the exit code lands in a file.
retry ferry sh "$builder" --detach "[ -f ~/$remote/build.log ] && exit 0; $jdk cd ~/$remote/android && ./gradlew --no-daemon -q :app:assemblePreview -PhexPreviewBuild=$build \
  -PhexPreviewChannel=$raw > ~/$remote/build.log 2>&1; echo \$? > ~/$remote/build.exit" >/dev/null
for _ in $(seq 1 240); do ferry sh "$builder" "test -f ~/$remote/build.exit" 2>/dev/null && break; sleep 15; done
if [ "$(retry ferry sh "$builder" "cat ~/$remote/build.exit")" != 0 ]; then
  ferry sh "$builder" "tail -40 ~/$remote/build.log" >&2; echo "the build failed on $builder" >&2; exit 1
fi
apk=HexPreview-$name.apk
out=$root/android/build/preview/$build; mkdir -p "$out"
retry ferry cp "$builder:$remote/android/app/build/outputs/apk/preview/app-preview.apk" "$out/$apk" >/dev/null

# Check what was built before anything is published.
tools=$(retry ferry sh "$builder" 'ls -d ~/Library/Android/sdk/build-tools/36.0.0')
signer=$(retry ferry sh "$builder" "$jdk $tools/apksigner verify --print-certs ~/$remote/android/app/build/outputs/apk/preview/app-preview.apk" | sed -n 's/.*SHA-256 digest: //p' | tr a-f A-F)
badging=$(retry ferry sh "$builder" "$tools/aapt2 dump badging ~/$remote/android/app/build/outputs/apk/preview/app-preview.apk" | head -1)
[ "$signer" = "$fingerprint" ] || { echo "APK is signed with $signer, not the Hex Preview key" >&2; exit 1; }
case "$badging" in *"name='ly.anoma.hex.preview' versionCode='$build' versionName='$name'"*) ;; *) echo "unexpected APK: $badging" >&2; exit 1 ;; esac
sha=$(shasum -a 256 "$out/$apk" | cut -d' ' -f1); size=$(stat -f %z "$out/$apk")
echo "$sha  $apk" > "$out/$apk.sha256"
url=https://github.com/$repo/releases/download/$tag/$apk

python3 - "$notes" "$out" <<PY
import json, sys, datetime
notes = [l.strip() for l in open(sys.argv[1]) if l.strip()]
latest = {"versionCode": $build, "versionName": "$name", "version": "$version", "minSdk": $minsdk,
          "published": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%d"), "commit": "$commit",
          "apk": {"url": "$url", "size": $size, "sha256": "$sha"}, "notes": notes}
open(sys.argv[2] + "/latest.json", "w").write(json.dumps(latest, indent=2, ensure_ascii=False) + "\n")
tick = chr(96)  # a literal backtick: this heredoc is expanded by bash
body = "\n".join("- " + n for n in notes) + "\n\nHex Preview for Android, built from ${commit:0:12}. Install it outside Google Play; it updates itself from [the channel](https://github.com/$repo/tree/$channel).\n\nAPK SHA-256: " + tick + "$sha" + tick + "  \nSigning certificate SHA-256: " + tick + "$fingerprint" + tick + "\n"
open(sys.argv[2] + "/release.md", "w").write(body)
PY

gh release create "$tag" --repo "$repo" --target "$commit" --prerelease --latest=false \
  --title "Hex Preview $version (build $build)" --notes-file "$out/release.md" "$out/$apk" "$out/$apk.sha256"
# The asset must serve exactly what was checked before the channel points at it.
curl -fsSL "$url" | shasum -a 256 | grep -q "^$sha " || { echo "the published APK doesn't match; channel left as it was" >&2; exit 1; }

# Point the channel at it: one commit on the channel branch, fast-forward only.
ch=$(mktemp -d)
git -C "$ch" init -q
git -C "$ch" remote add origin "https://github.com/$repo.git"
if git -C "$ch" fetch -q --depth 1 origin "$channel" 2>/dev/null; then git -C "$ch" checkout -q -b "$channel" FETCH_HEAD
else git -C "$ch" checkout -q --orphan "$channel"; fi
cp "$out/latest.json" "$ch/latest.json"
fp=$(echo "$fingerprint" | sed 's/../&:/g; s/:$//')
cat > "$ch/README.md" <<MD
# Hex Preview for Android

**[Download Hex Preview $version (build $build)]($url)** · $(( size / 1000000 )) MB · Android 12 or later · $(date -u +%Y-%m-%d)

Hex Preview is the [Hex keyboard](https://anoma.ly/hex) for Android, before it reaches Google Play. It installs as its own app beside Hex from Google Play, and it updates itself: it checks this page's \`latest.json\` when you open it and at most once a day from the keyboard, shows what changed, and installs a new build only when you tap Update.

## Install

1. Open the download link above on your phone and download the APK.
2. Open it. Android asks whether your browser or Files app may install apps: tap **Settings**, turn on **Allow from this source**, then go back and tap **Install**.
3. Open Hex Preview and follow its setup: the microphone, Hex Preview as your keyboard, and the 190 MB speech model.

The first update asks the same once for Hex Preview itself.

## Check a download

- APK SHA-256: \`$sha\`
- Signing certificate SHA-256: \`$fp\`

Every Hex Preview build is signed with this certificate; Android refuses an update signed with any other. Check with \`apksigner verify --print-certs\` or \`keytool -printcert -jarfile\`.

## What changed

$(sed '/^[[:space:]]*$/d; s/^/- /' "$notes")

Earlier builds are on the [releases page](https://github.com/$repo/releases?q=android-preview) (tags \`android-preview-<build>\`). Typing and dictation stay on the phone; the update check asks GitHub for this file and carries nothing about you.
MD
git -C "$ch" add latest.json README.md
git -C "$ch" -c user.name="$(git -C "$root" config user.name)" -c user.email="$(git -C "$root" config user.email)" \
  commit -q -m "Hex Preview $name (${commit:0:12})"
git -C "$ch" push -q origin "HEAD:refs/heads/$channel"
rm -rf "$ch"
echo "published $url"
echo "channel   https://github.com/$repo/tree/$channel"
echo "latest    $raw (GitHub's raw cache keeps an old copy up to 5 minutes)"
