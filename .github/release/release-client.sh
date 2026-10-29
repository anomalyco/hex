#!/usr/bin/env bash
# Vendored from https://anoma.ly/release/client.sh (sha256
# f19e3852cca5b675a0871487a035fc5ed42a871df429da10fd688568d1ad526e, 2026-10-29)
# so the publish job runs only locked code. Update it by copying a reviewed
# version here.
# The Anomaly release service client, served at https://anoma.ly/release/client.sh.
# Runs in a GitHub Actions job with `permissions: id-token: write`; needs bash,
# curl, jq, split and sha256sum (or shasum). See docs/publishing.md.
#
#   client.sh publish --app <app> --artifact <artifact> --release <release.json> \
#     --target <target>=<path>[=<published name>]... [--out <manifest.json>] [--verify-downloads]
#   client.sh distribution --app <app> --artifact <artifact> --channel <channel> --entry '<json>'
#   client.sh action --app <app> --artifact <artifact> --channel <channel> --action <name> [--body '<json>']
#
# It trades the job's GitHub OIDC token (which expires within minutes) for a
# one-hour publish session scoped to one app, artifact and channel, uploads in
# parallel (skipping files the service already has), retries with backoff and
# then publishes. `release.json` holds the release fields (channel, version,
# notes_url, distributions, …); the client fills in `targets`. RELEASE is the
# service origin (default https://anoma.ly/release).
set -euo pipefail

RELEASE=${RELEASE:-https://anoma.ly/release}
RELEASE=${RELEASE%/}
SINGLE_LIMIT=$((95 * 1024 * 1024))
PART_SIZE=90m
PARALLEL=${RELEASE_PARALLEL:-4}
ATTEMPTS=6

die() { echo "release: $*" >&2; exit 1; }
log() { echo "release: $*" >&2; }

for command in curl jq split; do command -v "$command" > /dev/null || die "needs $command"; done
if command -v sha256sum > /dev/null; then sha256_of() { sha256sum < "$1" | cut -d' ' -f1; }
elif command -v shasum > /dev/null; then sha256_of() { shasum -a 256 < "$1" | cut -d' ' -f1; }
else die "needs sha256sum or shasum"; fi
size_of() { wc -c < "$1" | tr -d ' '; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# request <body file> <curl arguments>: one call to the service with the
# session; writes the body to <body file> and sets STATUS. Network failures,
# 429 and 5xx are retried with exponential backoff; other answers return.
request() {
  local response=$1 attempt=1 delay=2 reason
  shift
  while :; do
    if STATUS=$(curl -sS --config "$work/session.curl" --speed-limit 1024 --speed-time 60 -o "$response" -w '%{http_code}' "$@"); then
      case "$STATUS" in 429 | 5??) ;; *) return 0 ;; esac
      reason="HTTP $STATUS: $(head -c 300 "$response")"
    else
      reason="request failed"
    fi
    [ "$attempt" -lt "$ATTEMPTS" ] || die "giving up after $attempt attempts ($reason): ${*: -1}"
    log "attempt $attempt ($reason); retrying in ${delay}s: ${*: -1}"
    sleep "$delay"
    attempt=$((attempt + 1))
    delay=$((delay * 2 > 60 ? 60 : delay * 2))
  done
}

# api <curl arguments>: a request that must succeed; prints its body.
api() {
  local response
  response=$(mktemp "$work/response.XXXXXX")
  request "$response" "$@"
  case "$STATUS" in 2??) cat "$response"; rm -f "$response" ;; *) die "HTTP $STATUS: $(head -c 1000 "$response") (${*: -1})" ;; esac
}

# session <app> <artifact> <channel>: trades the job's OIDC token for a
# publish session, kept in a curl config file (never in argv or the log).
session() {
  : "${ACTIONS_ID_TOKEN_REQUEST_URL:?run in a GitHub Actions job with id-token: write}"
  : "${ACTIONS_ID_TOKEN_REQUEST_TOKEN:?run in a GitHub Actions job with id-token: write}"
  local oidc body attempt
  umask 077
  for attempt in 1 2 3 4; do
    oidc=$(curl -fsS -H "Authorization: Bearer $ACTIONS_ID_TOKEN_REQUEST_TOKEN" "$ACTIONS_ID_TOKEN_REQUEST_URL&audience=$RELEASE" | jq -er .value) && break
    [ "$attempt" -lt 4 ] || die "could not get a GitHub OIDC token"
    sleep $((attempt * 2))
  done
  printf 'header = "Authorization: Bearer %s"\n' "$oidc" > "$work/session.curl"
  body=$(api -H 'content-type: application/json' -d "$(jq -cn --arg artifact "$2" --arg channel "$3" '{artifact: $artifact, channel: $channel}')" "$RELEASE/$1/sessions")
  printf 'header = "Authorization: Bearer %s"\n' "$(printf '%s' "$body" | jq -er .token)" > "$work/session.curl"
  log "publish session for $1/$2/$3 until $(printf '%s' "$body" | jq -r .expires_at)"
}

# upload <app> <path> <name>: stores one file unless the service already has
# exactly these bytes; refused by the service unless its sha256 matches.
upload() {
  local app=$1 path=$2 name=$3 sha256 size started id part chunk
  sha256=$(sha256_of "$path")
  size=$(size_of "$path")
  if curl -fsSI --retry 3 "$RELEASE/$app/files/$sha256/$name" > /dev/null 2>&1; then
    log "already stored: $name ($sha256)"
    return
  fi
  started=$(date +%s)
  if [ "$size" -le "$SINGLE_LIMIT" ]; then
    api -X PUT -H "x-checksum-sha256: $sha256" --data-binary @"$path" "$RELEASE/$app/files/$name" > /dev/null
  else
    id=$(api -X POST "$RELEASE/$app/files/$name?uploads" | jq -er .upload)
    mkdir -p "$work/parts/$id"
    split -b "$PART_SIZE" "$path" "$work/parts/$id/part."
    part=1
    for chunk in "$work/parts/$id"/part.*; do
      api -X PUT --data-binary @"$chunk" "$RELEASE/$app/files/$name?upload=$id&part=$part" > /dev/null
      part=$((part + 1))
    done
    api -X POST -H "x-checksum-sha256: $sha256" "$RELEASE/$app/files/$name?upload=$id" > /dev/null
  fi
  log "uploaded $name ($size bytes, $sha256) in $(($(date +%s) - started))s"
}

publish() {
  local app="" artifact="" release_file="" out="" verify=0 targets=() spec target path name pinned='{}' channel version body manifest key queued=""
  while [ $# -gt 0 ]; do
    case "$1" in
      --app) app=$2; shift 2 ;;
      --artifact) artifact=$2; shift 2 ;;
      --release) release_file=$2; shift 2 ;;
      --target) targets+=("$2"); shift 2 ;;
      --out) out=$2; shift 2 ;;
      --verify-downloads) verify=1; shift ;;
      *) die "unknown option $1" ;;
    esac
  done
  [ -n "$app" ] && [ -n "$artifact" ] && [ -f "$release_file" ] || die "publish needs --app, --artifact and --release <file>"
  [ "${#targets[@]}" -gt 0 ] || die "publish needs at least one --target"
  channel=$(jq -er .channel "$release_file")
  version=$(jq -er .version "$release_file")
  session "$app" "$artifact" "$channel"

  for spec in "${targets[@]}"; do
    target=${spec%%=*}
    path=${spec#*=}
    name=$(basename "$path")
    case "$path" in *=*) name=${path#*=}; path=${path%%=*} ;; esac
    [ -f "$path" ] || die "$target: $path does not exist"
    pinned=$(printf '%s' "$pinned" | jq --arg target "$target" --arg file "$name" --arg path "$path" --arg sha256 "$(sha256_of "$path")" --arg size "$(size_of "$path")" \
      '.[$target] = {file: $file, sha256: $sha256, size: ($size | tonumber), path: $path}')
    # Several targets may share one file; upload it once.
    key="$(sha256_of "$path")/$name"
    case " $queued " in *" $key "*) continue ;; esac
    queued="$queued $key"
    while [ "$(jobs -rp | wc -l)" -ge "$PARALLEL" ]; do wait -n || true; done
    { upload "$app" "$path" "$name" || touch "$work/failed"; } &
  done
  wait
  [ ! -e "$work/failed" ] || die "an upload failed"

  body=$(jq --argjson pinned "$pinned" '. + {targets: ($pinned | map_values({file, sha256}))}' "$release_file")
  printf '%s' "$body" > "$work/release.json"
  request "$work/published.json" -H 'content-type: application/json' --data-binary @"$work/release.json" "$RELEASE/$app/$artifact/releases"
  case "$STATUS" in
    2??) manifest=$(cat "$work/published.json") ;;
    *)
      # A retried or repeated publish: fine if the channel already has exactly this release.
      manifest=$(curl -fsS --retry 3 "$RELEASE/$app/$artifact/$channel.json") || die "HTTP $STATUS publishing $version: $(head -c 1000 "$work/published.json")"
      [ "$(printf '%s' "$manifest" | jq -r .release.version)" = "$version" ] || die "HTTP $STATUS publishing $version: $(head -c 1000 "$work/published.json")"
      log "$channel already has $version; checking it is these files"
      ;;
  esac

  for target in $(printf '%s' "$pinned" | jq -r 'keys[]'); do
    local entry expected url
    expected=$(printf '%s' "$pinned" | jq -c --arg target "$target" '.[$target]')
    entry=$(printf '%s' "$manifest" | jq -c --arg target "$target" '.release.targets[$target] // empty')
    [ -n "$entry" ] || die "the published release has no $target"
    [ "$(printf '%s' "$entry" | jq -r .sha256)" = "$(printf '%s' "$expected" | jq -r .sha256)" ] || die "$target: published sha256 differs from the local file"
    [ "$(printf '%s' "$entry" | jq -r .size)" = "$(printf '%s' "$expected" | jq -r .size)" ] || die "$target: published size differs from the local file"
    if [ "$verify" = 1 ]; then
      url="${RELEASE%/release}$(printf '%s' "$entry" | jq -r .url)"
      curl -fsS --retry 3 -o "$work/served" "$url"
      [ "$(sha256_of "$work/served")" = "$(printf '%s' "$expected" | jq -r .sha256)" ] || die "$target: the service serves different bytes at $url"
    fi
  done
  [ -z "$out" ] || printf '%s\n' "$manifest" > "$out"
  log "published $app/$artifact/$channel $version"
}

distribution() {
  local app="" artifact="" channel="" entry=""
  while [ $# -gt 0 ]; do
    case "$1" in
      --app) app=$2; shift 2 ;;
      --artifact) artifact=$2; shift 2 ;;
      --channel) channel=$2; shift 2 ;;
      --entry) entry=$2; shift 2 ;;
      *) die "unknown option $1" ;;
    esac
  done
  [ -n "$app" ] && [ -n "$artifact" ] && [ -n "$channel" ] && [ -n "$entry" ] || die "distribution needs --app, --artifact, --channel and --entry"
  session "$app" "$artifact" "$channel"
  api -H 'content-type: application/json' --data-binary "$entry" "$RELEASE/$app/$artifact/$channel/distribution" \
    | jq -e --argjson entry "$entry" '.release.distributions | any(.kind == $entry.kind and .version == $entry.version)' > /dev/null \
    || die "the service did not record $entry"
  log "recorded $(printf '%s' "$entry" | jq -r '"\(.kind) \(.version)"') on $app/$artifact/$channel"
}

action() {
  local app="" artifact="" channel="" name="" body='{}'
  while [ $# -gt 0 ]; do
    case "$1" in
      --app) app=$2; shift 2 ;;
      --artifact) artifact=$2; shift 2 ;;
      --channel) channel=$2; shift 2 ;;
      --action) name=$2; shift 2 ;;
      --body) body=$2; shift 2 ;;
      *) die "unknown option $1" ;;
    esac
  done
  [ -n "$app" ] && [ -n "$artifact" ] && [ -n "$channel" ] && [ -n "$name" ] || die "action needs --app, --artifact, --channel and --action"
  session "$app" "$artifact" "$channel"
  api -H 'content-type: application/json' --data-binary "$body" "$RELEASE/$app/$artifact/$channel/$name"
}

command=${1:-}
[ $# -gt 0 ] && shift
case "$command" in
  publish) publish "$@" ;;
  distribution) distribution "$@" ;;
  action) action "$@" ;;
  *) die "usage: client.sh publish|distribution|action … (see the header of this script)" ;;
esac
