#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
version=${HEX_VERSION:-$(cargo metadata --no-deps --format-version 1 --manifest-path "$root/Cargo.toml" | jq -r '.packages[0].version')}
arch=$(uname -m)
artifact=${HEX_SERVICE_ARTIFACT:-$root/dist/service/HEX-Service-$version-$arch.zip}
digest="$artifact.sha256"
install_root="$HOME/Library/Application Support/HEX Service Proof"
version_dir="$install_root/versions/$version"
installed="$version_dir/HEX Service.app"
runtime="$install_root/runtime"
staging="$install_root/.staging-$version-$$"
expected_team=${VOICE_CONTROL_TEAM_ID:-QC99C9JE59}
expected_identifier=com.kitlangton.hex.service
pid=

cleanup() {
  if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
    kill -INT "$pid" 2>/dev/null || true
    shutdown_attempts=100
    while kill -0 "$pid" 2>/dev/null && [ "$shutdown_attempts" -gt 0 ]; do
      shutdown_attempts=$((shutdown_attempts - 1))
      sleep 0.05
    done
    if kill -0 "$pid" 2>/dev/null; then
      kill -TERM "$pid" 2>/dev/null || true
      sleep 0.1
    fi
    if kill -0 "$pid" 2>/dev/null; then
      kill -KILL "$pid" 2>/dev/null || true
    fi
  fi
  rm -rf "$staging"
}
trap cleanup EXIT INT TERM

if [ "$#" -ne 0 ]; then
  echo "Usage: $0" >&2
  exit 1
fi
if [ ! -f "$artifact" ] || [ ! -f "$digest" ]; then
  echo "Prepare the service artifact first with ./scripts/prepare-service-app.sh." >&2
  exit 1
fi

(cd "$(dirname -- "$artifact")" && shasum -a 256 -c "$(basename -- "$digest")")
rm -rf "$staging"
mkdir -p "$staging" "$install_root/versions" "$runtime"
/usr/bin/ditto -x -k "$artifact" "$staging"
bundle="$staging/HEX Service.app"
codesign --verify --deep --strict --verbose=2 "$bundle"
xcrun stapler validate "$bundle"
spctl --assess --type execute --verbose=2 "$bundle"
team=$(codesign -dvv "$bundle" 2>&1 | sed -n 's/^TeamIdentifier=//p')
identifier=$(codesign -dvv "$bundle" 2>&1 | sed -n 's/^Identifier=//p')
if [ "$team" != "$expected_team" ] || [ "$identifier" != "$expected_identifier" ]; then
  echo "Unexpected service identity: $team / $identifier" >&2
  exit 1
fi

rm -rf "$version_dir"
mkdir -p "$version_dir"
/usr/bin/ditto "$bundle" "$installed"
rm -rf "$staging"
'/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister' \
  -f "$installed"
rm -f "$runtime/local-api.json"

/usr/bin/open -gj -n \
  --env "HEX_APPLICATION_SUPPORT_DIR=$runtime" \
  "$installed" --args service

discovery="$runtime/local-api.json"
attempts=100
while [ ! -f "$discovery" ]; do
  attempts=$((attempts - 1))
  if [ "$attempts" -eq 0 ]; then
    echo "Timed out waiting for service discovery." >&2
    exit 1
  fi
  sleep 0.05
done
port=$(jq -r .port "$discovery")
token=$(jq -r .token "$discovery")
pid=$(jq -r .pid "$discovery")
health=$(curl --fail --silent --show-error \
  -H "Authorization: Bearer $token" \
  "http://127.0.0.1:$port/health")
models=$(curl --fail --silent --show-error \
  -H "Authorization: Bearer $token" \
  "http://127.0.0.1:$port/models")
jq -e '.apiVersion == "1" and .version != null' >/dev/null <<EOF
$health
EOF
jq -e 'length > 0 and .[0].id == "parakeet_v2"' >/dev/null <<EOF
$models
EOF

kill -INT "$pid"
attempts=100
while [ -f "$discovery" ]; do
  attempts=$((attempts - 1))
  if [ "$attempts" -eq 0 ]; then
    echo "Timed out waiting for service shutdown." >&2
    exit 1
  fi
  sleep 0.05
done
pid=

echo "Service bootstrap smoke test passed: $installed"
