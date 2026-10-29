#!/usr/bin/env bash
# deploy-site.sh <dev|production> <dir>: deploys the site Worker from a
# verified Build Output artifact (<dir>, site/.cloudflare/output from Site's
# build job) with the cf CLI pinned in cf/package-lock.json, which the
# workflow installs (npm ci --ignore-scripts) before the token is in reach.
#
# The artifact was built from repository code without secrets, so everything
# in it is untrusted. The Worker's code and assets may change freely, but the
# config cf deploys must name exactly this mode's Worker, routes, and
# bindings: the deploy token can write every Worker on the account, and a
# binding grants the Worker access to its resource. Any other key, Worker, or
# Container is refused; changing them means changing this script.
. "$(dirname "$0")/lib.sh"

mode=$1
case "$mode" in
  dev)
    worker=hex-dev-site host=dev.anoma.ly bucket=hex-dev-files
    ;;
  production)
    worker=hex-production-site host=anoma.ly bucket=hex-production-files
    ;;
  *) die "Unknown site mode: $mode" ;;
esac
: "${CLOUDFLARE_API_TOKEN:?Set CLOUDFLARE_API_TOKEN}"
: "${CLOUDFLARE_ACCOUNT_ID:?Set CLOUDFLARE_ACCOUNT_ID}"

src=$(cd "$2" && pwd)
release=$(cd "$(dirname "$0")" && pwd)

# Only the Build Output files cf reads; nothing else is copied to where cf runs.
unexpected=$(cd "$src" && find . ! -type d ! -type f -o -type f ! -path ./SHA256SUMS \
  ! -path ./v0/config.json \
  ! -path ./v0/workers/default/worker.config.json \
  ! -path './v0/workers/default/bundle/*' \
  ! -path './v0/workers/default/assets/*')
[ -z "$unexpected" ] || die "Unexpected files in the site artifact: $unexpected"

jq -e --arg mode "$mode" '. == {buildContext: {isPreview: false, mode: $mode}}' \
  "$src/v0/config.json" > /dev/null || die "The site's root config is not a $mode build"

jq -e --arg worker "$worker" --arg host "$host" --arg bucket "$bucket" '
  (keys - ["name", "compatibilityDate", "compatibilityFlags", "assets", "triggers",
    "workersDev", "previewUrls", "env", "manifest"]) == []
  and .name == $worker
  and (.compatibilityDate | type) == "string"
  and ((.compatibilityFlags // []) | all(type == "string"))
  and .assets == {runWorkerFirst: true}
  and .triggers == [
    {type: "fetch", pattern: "\($host)/hex", zone: "anoma.ly"},
    {type: "fetch", pattern: "\($host)/hex/*", zone: "anoma.ly"}
  ]
  and .workersDev == false
  and .previewUrls == false
  and .env == {
    ASSETS: {type: "assets"},
    FILES: {type: "r2", name: $bucket},
    CANONICAL_HOST: {type: "text", value: $host}
  }
  and (.manifest | keys) == ["mainModule", "modules", "type"]
  and (.manifest.mainModule | test("^[A-Za-z0-9._-]+$"))
  and .manifest.modules == {}
' "$src/v0/workers/default/worker.config.json" > /dev/null ||
  die "The site's Worker config is not the $mode site Worker (.github/release/deploy-site.sh)"

work=$(mktemp -d)
mkdir -p "$work/.cloudflare/output/v0/workers/default"
cp "$src/v0/config.json" "$work/.cloudflare/output/v0/"
cp -R "$src/v0/workers/default/." "$work/.cloudflare/output/v0/workers/default/"

cf="$release/cf/node_modules/.bin/cf"
[ -x "$cf" ] || die "Install the pinned cf CLI first: npm ci --ignore-scripts in .github/release/cf"
cd "$work"
"$cf" deploy --prebuilt --mode "$mode"
